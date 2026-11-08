// TEST-ONLY MP-7MCP-01 contract tests. No product endpoint is enabled by these fixtures.
// They exercise the isolated reference model (test/support), not production MCP authorization.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import {
  applyBulkBatch, authorizeWrite, buildReviewSurface, canRead, DEFAULT_LIMITS, digestOf, effectiveLimits, ENGINE_CEILINGS,
  FACTORY_OWNER_CAPS, LIMIT_KEYS, ownerApproveBatch, ownerRestoreTombstone, ownerRollbackBatch, renewWorkerLease,
  reviewFullDiff, reviewItemDetail, RISK_POLICY_VERSION, setAutoApplyPolicy, setOwnerCaps, simulateWrite, stageBulkBatch,
  startBulkJob, validateGrant
} from './support/mp-7mcp-authority-model.mjs';

const OWNER = Object.freeze({ localUi: true, authenticated: true });

function fixture(operation = 'create_import', kind = 'memory') {
  const state = {
    vaultId: 'vault-A', audience: 'mind-place-mcp', epoch: 17, now: 1000,
    locked: false, paused: false, authorityReadable: true,
    receipts: new Map(), audit: [], writeLog: [], approvals: [], grants: [], readGrants: [], jobs: {}, batches: {},
    ownerPolicy: { version: 1, caps: {}, autoApply: { enabled: false } }, sources: {}, privacy: { redacted: [] },
    collections: {
      'curated-A': { layer: 'curated', audience: ['owner', 'client-A'] },
      'curated-B': { layer: 'curated', audience: ['owner', 'client-A'] },
      'shared-C': { layer: 'curated', audience: ['owner', 'client-A', 'client-B', 'local-L'] },
      'raw-A': { layer: 'raw', audience: ['owner', 'local-L'] },
      'raw-private': { layer: 'raw', private: true, audience: ['owner'] },
      'curated-owner': { layer: 'curated', audience: ['owner'] },
      'graph-A': { layer: 'derived', audience: ['owner'] }
    },
    clients: {
      'client-A': { enabled: true, tier: 'cloud', writePolicyRevision: 4, readPolicyRevision: 2 },
      'client-B': { enabled: true, tier: 'cloud', writePolicyRevision: 4, readPolicyRevision: 2 },
      'local-L': { enabled: true, tier: 'local', writePolicyRevision: 4, readPolicyRevision: 2 },
      'local-M': { enabled: true, tier: 'local', writePolicyRevision: 4, readPolicyRevision: 2 }
    },
    objects: {
      prior: { vaultId: 'vault-A', collectionId: 'curated-A', objectKind: 'memory', revision: 3, data: 'v3', history: [] },
      raw1: { vaultId: 'vault-A', collectionId: 'raw-A', objectKind: 'raw_capture', revision: 1, data: 'raw', history: [] }
    }
  };
  const auth = {
    valid: true, revoked: false, clientId: 'client-A', vaultId: 'vault-A', audience: 'mind-place-mcp',
    epoch: 17, credentialClass: 'cloud', transport: 'tunnel'
  };
  const local = { ...auth, clientId: 'local-L', credentialClass: 'local', transport: 'loopback' };
  const req = {
    operation, vaultId: 'vault-A', collectionId: 'curated-A', objectKind: kind,
    operationId: 'op-A', payload: 'p1'
  };
  let n = 0;
  const grant = (op = operation, collection = 'curated-A', client = 'client-A', k = kind, extra = {}) => {
    const g = {
      id: 'g' + (++n), enabled: true, revoked: false, clientId: client, vaultId: 'vault-A',
      collectionId: collection, objectKind: k, operation: op, mode: 'allow', policyRevision: 4,
      issuedAt: 0, expiresAt: client === 'local-L' ? null : 3000, ...extra
    };
    state.grants.push(g);
    return g;
  };
  const approve = (request, extra = {}) => {
    state.approvals.push({
      ownerApproved: true, clientId: auth.clientId, vaultId: 'vault-A', collectionId: request.collectionId,
      operation: request.operation, operationId: request.operationId, bodyDigest: digestOf(request),
      policyRevision: 4, epoch: 17, expiresAt: 3000, ...extra
    });
  };
  const readGrant = (collection, client = 'client-A') => state.readGrants.push({
    enabled: true, revoked: false, clientId: client, vaultId: 'vault-A', collectionId: collection, policyRevision: 2
  });
  return { state, auth, local, req, grant, approve, readGrant };
}
const code = (r) => r.code;

test('default deny, and granting write does not imply read', () => {
  const f = fixture();
  assert.equal(authorizeWrite(f.state, f.auth, f.req).code, 'DENIED');
  f.grant();
  assert.equal(authorizeWrite(f.state, f.auth, f.req).code, 'ALLOWED');
  assert.equal(canRead(f.state, f.auth, 'curated-A'), false);
  f.readGrant('curated-A');
  assert.equal(canRead(f.state, f.auth, 'curated-A'), true);
  assert.equal(canRead(f.state, f.auth, 'curated-B'), false);
});

test('each write operation and object kind requires a separate grant', () => {
  const f = fixture();
  f.grant('append');
  assert.equal(authorizeWrite(f.state, f.auth, f.req).code, 'DENIED');
  f.grant('create_import', 'curated-A', 'client-A', 'route');
  assert.equal(authorizeWrite(f.state, f.auth, f.req).code, 'DENIED');
  f.grant();
  assert.equal(authorizeWrite(f.state, f.auth, f.req).code, 'ALLOWED');
});

test('per-operation policy is allow / ask_first / deny with default deny', () => {
  const f = fixture();
  const g = f.grant();
  g.mode = 'deny';
  assert.equal(authorizeWrite(f.state, f.auth, f.req).code, 'DENIED');
  delete g.mode;
  assert.equal(authorizeWrite(f.state, f.auth, f.req).code, 'DENIED');
  g.mode = 'ask_first';
  assert.equal(authorizeWrite(f.state, f.auth, f.req).code, 'APPROVAL_REQUIRED');
  f.approve(f.req);
  assert.equal(simulateWrite(f.state, f.auth, f.req).code, 'COMMITTED');
  // The approval was consumed; the same owner approval cannot authorize a second operation.
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, operationId: 'op-B' }).code, 'APPROVAL_REQUIRED');
});

test('wrong collection, wrong vault and wrong kind are denied without object enumeration', () => {
  const f = fixture();
  f.grant();
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, collectionId: 'curated-B' }).code, 'DENIED');
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, vaultId: 'vault-B' }).code, 'DENIED');
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, objectKind: 'source' }).code, 'DENIED');
});

test('only the server-authenticated principal is trusted, not a claimed client name', () => {
  const f = fixture();
  f.grant();
  assert.equal(authorizeWrite(f.state, { ...f.auth, clientId: 'client-B' }, f.req).code, 'DENIED');
  assert.equal(authorizeWrite(f.state, { ...f.auth, valid: false }, f.req).code, 'DENIED');
  assert.equal(authorizeWrite(f.state, { ...f.auth, audience: 'other' }, f.req).code, 'DENIED');
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, clientId: 'client-B' }).code, 'ALLOWED');
});

test('Lock, pause, epoch mismatch, revoked client and write-policy change fail closed', () => {
  const f = fixture();
  f.grant();
  f.state.locked = true;
  assert.equal(authorizeWrite(f.state, f.auth, f.req).code, 'DENIED');
  f.state.locked = false; f.state.paused = true;
  assert.equal(authorizeWrite(f.state, f.auth, f.req).code, 'DENIED');
  f.state.paused = false; f.state.epoch += 1;
  assert.equal(authorizeWrite(f.state, f.auth, f.req).code, 'DENIED');
  f.state.epoch -= 1; f.state.clients['client-A'].revoked = true;
  assert.equal(authorizeWrite(f.state, f.auth, f.req).code, 'DENIED');
  f.state.clients['client-A'].revoked = false; f.state.clients['client-A'].writePolicyRevision += 1;
  assert.equal(authorizeWrite(f.state, f.auth, f.req).code, 'DENIED');
});

test('expired grants, missing payload and invalid operation are denied', () => {
  const f = fixture();
  f.grant();
  f.state.now = 3000;
  assert.equal(authorizeWrite(f.state, f.auth, f.req).code, 'DENIED');
  f.state.now = 1000;
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, payload: undefined }).code, 'DENIED');
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, operation: 'grant_admin' }).code, 'DENIED');
});

test('update requires current authoritative membership and exact revision', () => {
  const f = fixture('update');
  f.grant();
  const base = { ...f.req, objectId: 'prior', expectedRevision: 3 };
  assert.equal(authorizeWrite(f.state, f.auth, base).code, 'ALLOWED');
  assert.equal(authorizeWrite(f.state, f.auth, { ...base, expectedRevision: 2 }).code, 'CONFLICT');
  assert.equal(authorizeWrite(f.state, f.auth, { ...base, objectId: 'missing' }).code, 'DENIED');
  assert.equal(authorizeWrite(f.state, f.auth, { ...base, collectionId: 'curated-B' }).code, 'DENIED');
});

test('logical move requires both source and target exact move grants', () => {
  const f = fixture('move');
  const req = { ...f.req, objectId: 'prior', expectedRevision: 3, destinationCollectionId: 'curated-B' };
  f.grant('move', 'curated-A');
  assert.equal(authorizeWrite(f.state, f.auth, req).code, 'DENIED');
  f.grant('move', 'curated-B');
  assert.equal(authorizeWrite(f.state, f.auth, req).code, 'ALLOWED');
  assert.equal(simulateWrite(f.state, f.auth, req).code, 'COMMITTED');
  assert.equal(f.state.objects.prior.collectionId, 'curated-B');
  assert.equal(f.state.objects.prior.revision, 4);
});

test('tombstone needs its own grant and a server-side owner approval, never a client flag', () => {
  const f = fixture('delete');
  const req = { ...f.req, objectId: 'prior', expectedRevision: 3 };
  assert.equal(authorizeWrite(f.state, f.auth, { ...req, ownerConfirmedDestruction: true }).code, 'DENIED');
  f.grant();
  assert.equal(authorizeWrite(f.state, f.auth, req).code, 'APPROVAL_REQUIRED');
  assert.equal(authorizeWrite(f.state, f.auth, { ...req, ownerConfirmedDestruction: true }).code, 'APPROVAL_REQUIRED');
  f.approve(req);
  assert.equal(simulateWrite(f.state, f.auth, req).code, 'COMMITTED');
  assert.equal(f.state.objects.prior.tombstoned, true);
  assert.equal(f.state.objects.prior.data, 'v3', 'tombstone never removes content');
});

test('identical replay is one commit; changed body with the same operation id conflicts', () => {
  const f = fixture();
  f.grant();
  const first = simulateWrite(f.state, f.auth, f.req);
  assert.equal(first.code, 'COMMITTED');
  assert.equal(f.state.objects['obj:client-A:op-A'].revision, 1);
  assert.equal(simulateWrite(f.state, f.auth, f.req).code, 'REPLAY');
  assert.equal(f.state.objects['obj:client-A:op-A'].revision, 1);
  assert.equal(simulateWrite(f.state, f.auth, { ...f.req, payload: 'different' }).code, 'CONFLICT');
  assert.equal(f.state.receipts.size, 1);
  assert.equal(JSON.stringify(first).includes('p1'), false);
});

test('replayed update bypasses the old precondition only after a fresh grant recheck', () => {
  const f = fixture('update');
  const g = f.grant();
  const req = { ...f.req, objectId: 'prior', expectedRevision: 3 };
  assert.equal(simulateWrite(f.state, f.auth, req).code, 'COMMITTED');
  assert.equal(simulateWrite(f.state, f.auth, req).code, 'REPLAY');
  g.revoked = true;
  assert.equal(simulateWrite(f.state, f.auth, req).code, 'DENIED');
});

test('before-commit crash creates no active revision or receipt', () => {
  const f = fixture('update');
  f.grant();
  const req = { ...f.req, objectId: 'prior', expectedRevision: 3 };
  assert.equal(simulateWrite(f.state, f.auth, req, { fault: 'before-commit' }).code, 'ABORTED');
  assert.equal(f.state.objects.prior.revision, 3);
  assert.equal(f.state.receipts.size, 0);
});

test('epoch fence between admission and commit prevents mutation', () => {
  const f = fixture('update');
  f.grant();
  const req = { ...f.req, objectId: 'prior', expectedRevision: 3 };
  assert.equal(simulateWrite(f.state, f.auth, req, { beforeCommit(s) { s.epoch += 1; } }).code, 'DENIED');
  assert.equal(f.state.objects.prior.revision, 3);
  assert.equal(f.state.receipts.size, 0);
});

test('revocation fence between admission and commit prevents mutation', () => {
  const f = fixture('update');
  const g = f.grant();
  const req = { ...f.req, objectId: 'prior', expectedRevision: 3 };
  assert.equal(simulateWrite(f.state, f.auth, req, { beforeCommit() { g.revoked = true; } }).code, 'DENIED');
  assert.equal(f.state.objects.prior.revision, 3);
  assert.equal(f.state.receipts.size, 0);
});

test('committed but lost acknowledgement is reconciled by an authorized retry', () => {
  const f = fixture();
  f.grant();
  assert.equal(simulateWrite(f.state, f.auth, f.req, { fault: 'after-commit' }).code, 'UNCERTAIN');
  assert.equal(f.state.receipts.size, 1);
  assert.equal(simulateWrite(f.state, f.auth, f.req).code, 'REPLAY');
});

test('post-commit Lock prevents disclosing even the receipt; the commit remains durable', () => {
  const f = fixture();
  f.grant();
  assert.equal(simulateWrite(f.state, f.auth, f.req, { beforeDisclosure(s) { s.locked = true; } }).code, 'DENIED');
  assert.equal(f.state.receipts.size, 1);
  assert.equal(simulateWrite(f.state, f.auth, f.req).code, 'DENIED');
});

test('no operation beyond the closed set exists, including restore, purge, admin and filesystem', () => {
  const f = fixture();
  f.grant();
  for (const operation of ['restore', 'purge', 'hard_delete', 'raw_delete', 'root_approve', 'sql_execute', 'filesystem_write', 'backup_restore', 'unlock']) {
    assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, operation }).code, 'DENIED', operation);
  }
});

test('replay receipt is denied if the object moved outside the current collection scope', () => {
  const f = fixture('update');
  f.grant();
  const req = { ...f.req, objectId: 'prior', expectedRevision: 3 };
  assert.equal(simulateWrite(f.state, f.auth, req).code, 'COMMITTED');
  f.state.objects.prior.collectionId = 'curated-B';
  assert.equal(simulateWrite(f.state, f.auth, req).code, 'DENIED');
});

test('F2 approval is exact-client, exact-operation, exact-digest, epoch-bound and single-use', () => {
  const f = fixture('delete');
  f.grant();
  const req = { ...f.req, objectId: 'prior', expectedRevision: 3 };
  f.approve(req, { clientId: 'client-B' });
  assert.equal(authorizeWrite(f.state, f.auth, req).code, 'APPROVAL_REQUIRED');
  f.state.approvals.length = 0;
  f.approve(req, { operationId: 'other-op' });
  assert.equal(authorizeWrite(f.state, f.auth, req).code, 'APPROVAL_REQUIRED');
  f.state.approvals.length = 0;
  f.approve({ ...req, payload: 'tampered' });
  assert.equal(authorizeWrite(f.state, f.auth, req).code, 'APPROVAL_REQUIRED');
  f.state.approvals.length = 0;
  f.approve(req, { epoch: 16 });
  assert.equal(authorizeWrite(f.state, f.auth, req).code, 'APPROVAL_REQUIRED');
  f.state.approvals.length = 0;
  f.approve(req);
  assert.equal(simulateWrite(f.state, f.auth, req).code, 'COMMITTED');
  assert.equal(f.state.approvals[0].used, true);
  // A second tombstone with a new operation id needs a fresh approval even for an identical body.
  f.state.objects.prior = { ...f.state.objects.prior, tombstoned: false, revision: 9 };
  const again = { ...req, operationId: 'op-2', expectedRevision: 9 };
  assert.equal(authorizeWrite(f.state, f.auth, again).code, 'APPROVAL_REQUIRED');
});

test('F1 tombstoned objects are immutable over MCP; only the owner can restore', () => {
  const f = fixture('delete');
  f.grant(); f.grant('update'); f.grant('append'); f.grant('move'); f.grant('move', 'curated-B');
  const del = { ...f.req, objectId: 'prior', expectedRevision: 3 };
  f.approve(del);
  assert.equal(simulateWrite(f.state, f.auth, del).code, 'COMMITTED');
  const base = { ...f.req, objectId: 'prior', expectedRevision: 4, collectionId: 'curated-A' };
  for (const [operation, extra] of [['update', {}], ['append', {}], ['move', { destinationCollectionId: 'curated-B' }], ['delete', {}]]) {
    assert.equal(authorizeWrite(f.state, f.auth, { ...base, operation, ...extra, operationId: 'x-' + operation }).code, 'DENIED', operation);
  }
  assert.equal(f.state.objects.prior.tombstoned, true);
  assert.equal(ownerRestoreTombstone(f.state, { localUi: false, authenticated: true }, 'prior').code, 'DENIED');
  assert.equal(ownerRestoreTombstone(f.state, OWNER, 'prior').code, 'RESTORED');
  assert.equal(f.state.objects.prior.tombstoned, false);
  assert.equal(f.state.objects.prior.history.length, 2, 'every revision is retained');
  assert.equal(authorizeWrite(f.state, f.auth, { ...base, operation: 'update', expectedRevision: 5, operationId: 'after' }).code, 'ALLOWED');
});

test('F3 raw evidence is immutable: only create_import and append, even with a mis-issued grant', () => {
  const f = fixture('update', 'raw_capture');
  f.grant('update', 'raw-A', 'local-L', 'raw_capture');
  f.grant('move', 'raw-A', 'local-L', 'raw_capture');
  f.grant('delete', 'raw-A', 'local-L', 'raw_capture');
  const base = { ...f.req, collectionId: 'raw-A', objectKind: 'raw_capture', objectId: 'raw1', expectedRevision: 1 };
  for (const operation of ['update', 'move', 'delete']) {
    assert.equal(authorizeWrite(f.state, f.local, { ...base, operation, destinationCollectionId: 'raw-A' }).code, 'DENIED', operation);
  }
  f.grant('append', 'raw-A', 'local-L', 'raw_capture');
  assert.equal(authorizeWrite(f.state, f.local, { ...base, operation: 'append' }).code, 'ALLOWED');
  f.grant('create_import', 'raw-A', 'local-L', 'raw_capture');
  assert.equal(authorizeWrite(f.state, f.local, { ...f.req, operation: 'create_import', collectionId: 'raw-A', objectKind: 'raw_capture' }).code, 'ALLOWED');
  const local = f.state.clients['local-L'];
  assert.deepEqual(validateGrant({ ...f.state.grants[0] }, local, 1000, f.state), { ok: false, reason: 'ineligible' });
});

test('F4 create_import ids are server-assigned; a caller-chosen id is denied opaquely', () => {
  const f = fixture();
  f.state.objects.other = { vaultId: 'vault-A', collectionId: 'curated-B', objectKind: 'memory', revision: 1, data: 'x', history: [] };
  f.grant();
  const hit = authorizeWrite(f.state, f.auth, { ...f.req, objectId: 'other' });
  const miss = authorizeWrite(f.state, f.auth, { ...f.req, objectId: 'does-not-exist' });
  assert.equal(hit.code, 'DENIED');
  assert.deepEqual(hit, miss, 'existing and non-existing ids are indistinguishable');
  assert.equal(simulateWrite(f.state, f.auth, f.req).code, 'COMMITTED');
  assert.ok(f.state.objects['obj:client-A:op-A']);
});

test('F5 editing write policy does not disturb read grants of read-only clients', () => {
  const f = fixture();
  f.readGrant('curated-A');
  assert.equal(canRead(f.state, f.auth, 'curated-A'), true);
  f.state.clients['client-A'].writePolicyRevision += 1;
  assert.equal(canRead(f.state, f.auth, 'curated-A'), true);
  f.state.clients['client-A'].readPolicyRevision += 1;
  assert.equal(canRead(f.state, f.auth, 'curated-A'), false);
});

test('F6 time, epoch, policy revision and digest are server-derived; request values are ignored', () => {
  const f = fixture('update');
  const g = f.grant();
  g.expiresAt = 10;
  const req = { ...f.req, objectId: 'prior', expectedRevision: 3 };
  assert.equal(authorizeWrite(f.state, f.auth, { ...req, now: 1, epoch: 17, policyRevision: 4 }).code, 'DENIED');
  g.expiresAt = 3000;
  const first = simulateWrite(f.state, f.auth, { ...req, bodyDigest: 'f'.repeat(64) });
  assert.equal(first.receipt.bodyDigest, digestOf(req));
  assert.equal(simulateWrite(f.state, f.auth, { ...req, payload: 'changed', bodyDigest: first.receipt.bodyDigest }).code, 'CONFLICT');
});

test('F7 every decision is audited, content-free, including denials', () => {
  const f = fixture();
  f.grant();
  simulateWrite(f.state, f.auth, { ...f.req, payload: 'secret-content-xyz' });
  simulateWrite(f.state, f.auth, { ...f.req, operationId: 'op-denied', collectionId: 'curated-B', payload: 'secret-content-xyz' });
  assert.deepEqual(f.state.audit.map((a) => [a.decision, a.code]), [['allow', 'COMMITTED'], ['deny', 'DENIED']]);
  assert.equal(JSON.stringify(f.state.audit).includes('secret-content-xyz'), false);
  assert.equal(JSON.stringify(f.state.audit).includes('p1'), false);
});

test('F7 cloud limits: 32 KiB, 20 objects, 60 writes per hour by default; overrides stay within the owner cap', () => {
  const f = fixture();
  const g = f.grant();
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, payload: 'x'.repeat(DEFAULT_LIMITS.maxBytes + 1) }).code, 'LIMIT');
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, objectCount: DEFAULT_LIMITS.maxObjects + 1 }).code, 'LIMIT');
  g.limits = { maxBytes: 40_000 };
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, payload: 'x'.repeat(DEFAULT_LIMITS.maxBytes + 1) }).code, 'ALLOWED');
  g.limits = { maxWritesPerHour: 2 };
  assert.equal(simulateWrite(f.state, f.auth, { ...f.req, operationId: 'w1' }).code, 'COMMITTED');
  assert.equal(simulateWrite(f.state, f.auth, { ...f.req, operationId: 'w2' }).code, 'COMMITTED');
  assert.equal(simulateWrite(f.state, f.auth, { ...f.req, operationId: 'w3' }).code, 'LIMIT');
  f.state.now += 3_600_001; g.expiresAt = f.state.now + 1000; g.issuedAt = f.state.now;
  assert.equal(simulateWrite(f.state, f.auth, { ...f.req, operationId: 'w4' }).code, 'COMMITTED');
});

test('F7 grant issuance rejects wildcards, empty values, unknown modes and cloud bulk/raw/long expiry', () => {
  const f = fixture();
  const cloud = f.state.clients['client-A'];
  const good = f.grant();
  assert.deepEqual(validateGrant(good, cloud, 1000, f.state), { ok: true });
  for (const [patch, reason] of [
    [{ collectionId: '*' }, 'field:collectionId'], [{ collectionId: 'curated-' + '*' }, 'field:collectionId'],
    [{ collectionId: '' }, 'field:collectionId'], [{ clientId: 'a b' }, 'field:clientId'],
    [{ operation: 'grant_admin' }, 'operation'], [{ mode: 'always' }, 'mode'], [{ objectKind: 'unknown' }, 'kind'],
    [{ operation: 'bulk_stage' }, 'cloud-bulk'], [{ objectKind: 'graph_batch' }, 'cloud-layer'],
    [{ expiresAt: DEFAULT_LIMITS.maxGrantMs + 1 }, 'cloud-expiry'], [{ expiresAt: null }, 'cloud-expiry']
  ]) {
    assert.deepEqual(validateGrant({ ...good, ...patch }, cloud, 1000, f.state), { ok: false, reason }, JSON.stringify(patch));
  }
  assert.deepEqual(validateGrant({ ...good, expiresAt: DEFAULT_LIMITS.maxGrantMs }, cloud, 1000, f.state), { ok: true });
});

test('F8 legacy read/write/admin capability bits confer no MCP write authority', () => {
  const f = fixture();
  f.state.clients['client-A'].legacyCapabilities = ['read', 'write', 'admin'];
  assert.equal(authorizeWrite(f.state, f.auth, f.req).code, 'DENIED');
  assert.equal(canRead(f.state, f.auth, 'curated-A'), false);
});

test('F8 receipts are bound to the principal: another client cannot borrow or replay them', () => {
  const f = fixture();
  f.grant();
  assert.equal(simulateWrite(f.state, f.auth, f.req).code, 'COMMITTED');
  const b = { ...f.auth, clientId: 'client-B' };
  assert.equal(simulateWrite(f.state, b, f.req).code, 'DENIED', 'client-B has no grant and gets no receipt');
  f.grant('create_import', 'curated-A', 'client-B');
  assert.equal(simulateWrite(f.state, b, f.req).code, 'COMMITTED', 'a separate receipt, not a replay');
  assert.equal(f.state.receipts.size, 2);
});

test('F8 an unreadable authority store fails closed for writes, reads and jobs', () => {
  const f = fixture();
  f.grant(); f.readGrant('curated-A');
  f.state.authorityReadable = false;
  assert.equal(authorizeWrite(f.state, f.auth, f.req).code, 'DENIED');
  assert.equal(canRead(f.state, f.auth, 'curated-A'), false);
});

test('tiers: cloud sees only curated; raw is local-only; local credentials never work through the tunnel', () => {
  const f = fixture();
  f.readGrant('curated-A'); f.readGrant('raw-A'); f.readGrant('raw-A', 'local-L');
  assert.equal(canRead(f.state, f.auth, 'curated-A'), true);
  assert.equal(canRead(f.state, f.auth, 'raw-A'), false, 'cloud never reads raw');
  assert.equal(canRead(f.state, f.local, 'raw-A'), true);
  assert.equal(canRead(f.state, { ...f.local, transport: 'tunnel' }, 'raw-A'), false);
  assert.equal(canRead(f.state, { ...f.auth, credentialClass: 'local' }, 'curated-A'), false, 'credential class must match the client tier');
  assert.equal(canRead(f.state, { ...f.local, credentialClass: 'cloud' }, 'raw-A'), false);
  f.grant('create_import', 'raw-A', 'client-A', 'raw_capture');
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, collectionId: 'raw-A', objectKind: 'raw_capture' }).code, 'DENIED');
  f.grant('create_import', 'curated-A', 'local-L');
  assert.equal(authorizeWrite(f.state, f.local, f.req).code, 'ALLOWED');
  assert.equal(authorizeWrite(f.state, { ...f.local, transport: 'tunnel' }, f.req).code, 'DENIED');
});

// ---- local tier: bulk jobs, limits (C1), lifetimes (C2) and review by exception (C3) ----
const DAY = 86_400_000;
function bulk(opts = {}) {
  const f = fixture('update');
  const target = opts.target ?? 'shared-C';
  f.grant('bulk_stage', target, 'local-L', 'memory', opts.stageExtra);
  f.grant('bulk_apply', target, 'local-L', 'memory', { mode: opts.applyMode ?? 'ask_first', ...(opts.applyExtra ?? {}) });
  for (const op of ['create_import', 'append', 'update']) f.grant(op, target, 'local-L', 'memory');
  f.readGrant('raw-A', 'local-L'); f.readGrant('curated-A', 'local-L'); f.readGrant('raw-private', 'local-L');
  f.state.objects.m1 = { vaultId: 'vault-A', collectionId: target, objectKind: 'memory', revision: 2, data: 'old', history: [] };
  const job = { jobId: 'job-1', targetCollectionId: target, objectKind: 'memory', scopeCollections: opts.scope ?? ['raw-A'] };
  // Evidence pointers resolve against state.sources and carry the content digest seen at staging time.
  const src = (ref, collectionId = 'raw-A') => {
    const key = collectionId + '#' + ref;
    f.state.sources[key] ??= { digest: 'digest-' + ref };
    return { collectionId, ref, digest: f.state.sources[key].digest };
  };
  const clusters = [{ id: 'c1', theme: 'Mind Place architecture' }];
  const items = (extra = {}) => [
    { operation: 'create_import', payload: 'new curated note', sources: ['raw-A'], evidence: [src('msg-1')], cluster: 'c1', ...extra },
    { operation: 'update', objectId: 'm1', expectedRevision: 2, payload: 'merged', sources: ['raw-A'], evidence: [src('msg-2')], cluster: 'c1' }
  ];
  const creates = (n, p = 'n') => Array.from({ length: n }, (_, i) => ({ operation: 'create_import', payload: p + i, sources: ['raw-A'], evidence: [src(p + 'r' + i)], cluster: 'c1' }));
  const start = (j = job) => startBulkJob(f.state, f.local, j);
  const stage = (batchId = 'b1', its = items(), meta = { clusters }, jobId = 'job-1') => stageBulkBatch(f.state, f.local, jobId, batchId, its, meta);
  const surface = (batchId = 'b1') => buildReviewSurface(f.state, 'local-L', batchId);
  const approve = (batchId = 'b1', decisions = {}, summaryDigest) =>
    ownerApproveBatch(f.state, OWNER, 'local-L', batchId, { summaryDigest: summaryDigest ?? surface(batchId).summaryDigest, decisions });
  const apply = (batchId = 'b1', o) => applyBulkBatch(f.state, f.local, 'job-1', batchId, o);
  const objects = () => structuredClone(f.state.objects);
  const grantOf = (op) => f.state.grants.find((g) => g.operation === op && g.clientId === 'local-L');
  const autoApply = (extra = {}) => setAutoApplyPolicy(f.state, OWNER, { enabled: true, maxRisk: 'low', maxItems: 100, clients: ['local-L'], ...extra });
  return { ...f, job, items, creates, clusters, src, start, stage, surface, approve, apply, objects, grantOf, autoApply, target };
}

test('bulk: cloud clients cannot start bulk jobs or smuggle bulk through single writes', () => {
  const b = bulk();
  b.grant('bulk_stage', 'curated-A', 'client-A');
  assert.equal(startBulkJob(b.state, b.auth, { ...b.job, targetCollectionId: 'curated-A' }).code, 'DENIED');
  b.grant('create_import', 'curated-A', 'client-A');
  assert.equal(authorizeWrite(b.state, b.auth, { ...b.req, operation: 'create_import', bulk: true }).code, 'DENIED');
  assert.equal(startBulkJob(b.state, { ...b.local, transport: 'tunnel' }, b.job).code, 'DENIED');
});

test('bulk: staged proposals are invisible until a separate apply; evidence, scope and clusters are mandatory', () => {
  const b = bulk();
  assert.equal(b.start().code, 'STARTED');
  const before = b.objects();
  assert.equal(b.stage('bx', b.items().map((i) => ({ ...i, evidence: [] }))).code, 'DENIED');
  assert.equal(b.stage('bx', b.items({ sources: ['curated-A'] })).code, 'DENIED', 'sources must stay inside the declared scope');
  assert.equal(b.stage('bx', b.items().map(({ cluster, ...i }) => i)).code, 'INVALID', 'every item belongs to a cluster');
  assert.equal(b.stage('bx', b.items(), { clusters: [] }).code, 'INVALID');
  assert.equal(b.stage('b1').code, 'STAGED');
  assert.deepEqual(b.objects(), before, 'nothing is visible before apply');
  assert.equal(b.surface().evidenceCount, 2);
});

test('bulk: deterministic deduplication rejects identical payloads and no-op updates', () => {
  const b = bulk();
  b.start();
  const dup = [...b.creates(1), { ...b.creates(1)[0], evidence: [b.src('other')] }];
  assert.equal(b.stage('bd', dup).code, 'DUPLICATE');
  const noop = [{ operation: 'update', objectId: 'm1', expectedRevision: 2, payload: 'old', sources: ['raw-A'], evidence: [b.src('m')], cluster: 'c1' }];
  assert.equal(b.stage('bn', noop).code, 'DUPLICATE');
});

test('bulk: apply needs one short-lived, single-use approval bound to the displayed batch', () => {
  const b = bulk();
  b.start();
  const staged = b.stage();
  assert.equal(b.apply().code, 'APPROVAL_REQUIRED');
  assert.equal(b.approve('b1', {}, 'f'.repeat(64)).code, 'STALE_SUMMARY', 'the owner must approve the summary that is shown now');
  assert.equal(b.approve().code, 'APPROVED');
  assert.equal(b.surface().batchDigest, staged.batchDigest);
  const applied = b.apply();
  assert.deepEqual([applied.code, applied.created, applied.changed, applied.withheld, applied.auto], ['APPLIED', 1, 1, 0, false]);
  assert.equal(b.state.objects.m1.data, 'merged');
  assert.equal(b.state.objects.m1.revision, 3);
  assert.equal(b.apply().code, 'DENIED', 'a batch applies once');
  assert.equal(b.state.approvals.at(-1).used, true);
});

test('bulk apply re-validates the declared scope at commit: a source revoked after staging blocks the batch', () => {
  const cases = {
    'read grant revoked': (s) => { s.readGrants.find((g) => g.clientId === 'local-L' && g.collectionId === 'raw-A').revoked = true; },
    'read policy revision bumped': (s) => { s.clients['local-L'].readPolicyRevision += 1; },
    'source became private for a broader target': (s) => { s.collections['raw-A'].private = true; s.collections['raw-A'].audience = ['owner']; },
    'source collection removed': (s) => { delete s.collections['raw-A']; }
  };
  for (const [label, mutate] of Object.entries(cases)) {
    for (const when of ['before apply', 'inside commit']) {
      const b = bulk();
      b.start(); b.stage(); b.approve();
      const expected = b.objects();
      const res = when === 'before apply' ? (mutate(b.state), b.apply()) : b.apply('b1', { beforeCommit: mutate });
      assert.equal(res.code, 'DENIED', label + ' / ' + when);
      assert.deepEqual(b.state.objects, expected, label + ' / ' + when + ': nothing applied');
      assert.equal(b.state.batches['local-L:b1'].status, 'staged');
    }
  }
});

test('bulk: ask_first on bulk_apply or any contained operation always needs the owner approval, even for auto-apply eligible batches', () => {
  const b = bulk({ applyMode: 'allow' });
  assert.equal(b.autoApply().code, 'AUTO_APPLY_SET');
  b.start(); b.stage('b1', b.creates(2));
  assert.equal(b.surface().autoApplyEligible, true);
  b.grantOf('create_import').mode = 'ask_first';
  assert.equal(b.surface().autoApplyEligible, false, 'strictest policy wins');
  assert.equal(b.apply().code, 'APPROVAL_REQUIRED');
  b.grantOf('create_import').mode = 'allow';
  b.grantOf('append').mode = 'ask_first'; // an operation that is not in the batch is irrelevant
  assert.equal(b.apply().code, 'APPLIED');
});

test('bulk: tightening a mode between stage and commit takes effect and applies nothing', () => {
  const b = bulk({ applyMode: 'allow' });
  b.autoApply();
  b.start(); b.stage('b1', b.creates(2));
  const expected = b.objects();
  assert.equal(b.apply('b1', { beforeCommit() { b.grantOf('create_import').mode = 'ask_first'; } }).code, 'APPROVAL_REQUIRED');
  assert.deepEqual(b.state.objects, expected);
});

test('bulk: revoke, Lock or epoch change between stage and apply denies the whole batch', () => {
  for (const mutate of [
    (s) => { s.epoch += 1; }, (s) => { s.locked = true; }, (s) => { s.clients['local-L'].writePolicyRevision += 1; },
    (s) => { s.grants.find((g) => g.operation === 'bulk_apply').revoked = true; }
  ]) {
    const b = bulk();
    b.start(); b.stage(); b.approve();
    mutate(b.state);
    assert.equal(b.apply().code, 'DENIED');
    assert.equal(b.state.objects.m1.revision, 2);
  }
});

test('bulk apply is atomic: one stale item applies nothing', () => {
  const b = bulk();
  b.start(); b.stage(); b.approve();
  const expected = b.objects();
  expected.m1.revision = 3; // the only change: the concurrent edit injected below
  const res = b.apply('b1', { beforeCommit(s) { s.objects.m1.revision = 3; } });
  assert.equal(res.code, 'CONFLICT');
  assert.deepEqual(b.state.objects, expected, 'no item of the batch was applied');
  assert.equal(b.state.batches['local-L:b1'].status, 'staged');
});

test('bulk: tombstone and move are never bulk operations', () => {
  const b = bulk();
  b.grant('delete', b.target, 'local-L', 'memory'); b.grant('move', b.target, 'local-L', 'memory');
  b.start();
  for (const operation of ['delete', 'move']) {
    const items = [{ operation, objectId: 'm1', expectedRevision: 2, payload: '', sources: ['raw-A'], evidence: [b.src('r')], cluster: 'c1' }];
    assert.equal(b.stage('b-' + operation, items).code, 'DENIED', operation);
  }
});

test('bulk rollback is owner-only and logical: created objects are tombstoned, changed objects get a new revision', () => {
  const b = bulk();
  b.start(); b.stage(); b.approve(); b.apply();
  const createdId = 'obj:local-L:b1:0';
  assert.equal(ownerRollbackBatch(b.state, { localUi: false, authenticated: true }, 'local-L', 'b1').code, 'DENIED');
  assert.equal(ownerRollbackBatch(b.state, OWNER, 'local-L', 'b1').code, 'ROLLED_BACK');
  assert.equal(b.state.objects[createdId].tombstoned, true);
  assert.equal(b.state.objects[createdId].data, 'new curated note', 'content is retained');
  assert.equal(b.state.objects.m1.data, 'old');
  assert.equal(b.state.objects.m1.revision, 4, 'rollback is a new revision, history is kept');
  assert.equal(b.state.objects.m1.history.length, 2);
  assert.equal(ownerRollbackBatch(b.state, OWNER, 'local-L', 'b1').code, 'DENIED', 'rolled back once');
});

test('bulk rollback refuses to clobber later edits', () => {
  const b = bulk();
  b.start(); b.stage(); b.approve(); b.apply();
  b.state.objects.m1 = { ...b.state.objects.m1, revision: 9, data: 'edited later' };
  assert.equal(ownerRollbackBatch(b.state, OWNER, 'local-L', 'b1').code, 'CONFLICT');
  assert.equal(b.state.objects.m1.data, 'edited later');
  assert.equal(b.state.objects['obj:local-L:b1:0'].tombstoned, false, 'all-or-nothing');
});

test('sub-agent and model choice carry no authority of their own', () => {
  const b = bulk();
  b.start();
  const items = b.items().map((i) => ({ ...i, subagent: 'haiku-worker', actingAs: 'owner' }));
  assert.equal(b.stage('b1', items).code, 'STAGED');
  assert.equal(stageBulkBatch(b.state, { ...b.local, clientId: 'client-A', credentialClass: 'cloud', transport: 'tunnel' }, 'job-1', 'b2', b.items(), { clusters: b.clusters }).code, 'DENIED');
  assert.equal(b.state.audit.at(-1).clientId, 'client-A');
});

// ---------------- C1: safe defaults < owner hard caps < engine ceilings ----------------
test('C1 the limit model is well ordered and never exceeds the current production request envelope', () => {
  for (const k of LIMIT_KEYS) {
    assert.ok(DEFAULT_LIMITS[k] <= FACTORY_OWNER_CAPS[k], k + ': default <= owner cap');
    assert.ok(FACTORY_OWNER_CAPS[k] <= ENGINE_CEILINGS[k], k + ': owner cap <= engine ceiling');
  }
  assert.equal(DEFAULT_LIMITS.maxConcurrentWorkers, 3);
  assert.equal(FACTORY_OWNER_CAPS.maxConcurrentWorkers, 5);
  assert.ok(ENGINE_CEILINGS.maxBytes < 64 * 1024, 'payload ceiling stays inside the 64 KiB MCP request envelope (BODY_LIMIT)');
  assert.ok(Object.isFrozen(ENGINE_CEILINGS) && Object.isFrozen(DEFAULT_LIMITS) && Object.isFrozen(FACTORY_OWNER_CAPS));
});

test('C1 owner caps are owner-only, atomic, bounded by default and ceiling, and audited', () => {
  const f = fixture();
  assert.equal(setOwnerCaps(f.state, { localUi: false, authenticated: true }, { maxObjects: 30 }).code, 'DENIED');
  assert.equal(setOwnerCaps(f.state, { localUi: true, authenticated: false }, { maxObjects: 30 }).code, 'DENIED');
  const before = structuredClone(f.state.ownerPolicy);
  for (const caps of [
    { maxObjects: DEFAULT_LIMITS.maxObjects - 1 }, { maxObjects: ENGINE_CEILINGS.maxObjects + 1 },
    { maxBytes: 1_048_576 }, { maxBytes: ENGINE_CEILINGS.maxBytes + 1 }, { maxObjects: -5 }, { maxObjects: 0 }, { maxObjects: NaN },
    { maxObjects: Infinity }, { maxObjects: 30.5 }, { maxObjects: '30' }, { maxObjects: null }, { maxEverything: 5 },
    { maxConcurrentWorkers: ENGINE_CEILINGS.maxConcurrentWorkers + 1 }, { maxConcurrentWorkers: DEFAULT_LIMITS.maxConcurrentWorkers - 1 },
    {}, [], null, 'x'
  ]) {
    assert.equal(setOwnerCaps(f.state, OWNER, caps).code, 'INVALID', JSON.stringify(caps));
    assert.deepEqual(f.state.ownerPolicy, before, 'invalid input changes nothing');
  }
  assert.equal(setOwnerCaps(f.state, OWNER, { maxObjects: 60, maxBytes: 1_048_576 }).code, 'INVALID', 'atomic: one bad value rejects all');
  assert.equal(setOwnerCaps(f.state, OWNER, { maxObjects: 60, maxConcurrentWorkers: 8 }).code, 'CAPS_SET');
  assert.deepEqual([f.state.ownerPolicy.caps, f.state.ownerPolicy.version], [{ maxObjects: 60, maxConcurrentWorkers: 8 }, 2]);
  assert.equal(f.state.audit.at(-1).code, 'CAPS_SET');
  assert.equal(effectiveLimits(f.state, undefined).caps.maxObjects, 60);
});

test('C1 overrides above the owner cap are rejected at issuance, clamped and fail-closed at enforcement', () => {
  const f = fixture();
  const cloud = f.state.clients['client-A'];
  const g = f.grant();
  const v = (limits) => validateGrant({ ...g, limits }, cloud, 1000, f.state);
  assert.deepEqual(v({ maxObjects: FACTORY_OWNER_CAPS.maxObjects }), { ok: true });
  assert.deepEqual(v({ maxObjects: FACTORY_OWNER_CAPS.maxObjects + 1 }), { ok: false, reason: 'limit-over-cap:maxObjects' });
  assert.deepEqual(v({ maxBytes: FACTORY_OWNER_CAPS.maxBytes + 1 }), { ok: false, reason: 'limit-over-cap:maxBytes' });
  assert.deepEqual(v({ maxObjects: -1 }), { ok: false, reason: 'limit-invalid:maxObjects' });
  assert.deepEqual(v({ maxObjects: 0 }), { ok: false, reason: 'limit-invalid:maxObjects' });
  assert.deepEqual(v({ maxObjects: 2.5 }), { ok: false, reason: 'limit-invalid:maxObjects' });
  assert.deepEqual(v({ maxObjects: '9' }), { ok: false, reason: 'limit-invalid:maxObjects' });
  assert.deepEqual(v({ maxEverything: 9 }), { ok: false, reason: 'limit-key:maxEverything' });
  assert.deepEqual(v([1]), { ok: false, reason: 'limits' });
  assert.deepEqual(v(null), { ok: false, reason: 'limits' });
  // A record that bypassed the validator (tampering, migration bug) is clamped to the CURRENT cap at enforcement...
  g.limits = { maxObjects: 10_000 };
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, objectCount: FACTORY_OWNER_CAPS.maxObjects + 1 }).code, 'LIMIT');
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, objectCount: FACTORY_OWNER_CAPS.maxObjects }).code, 'ALLOWED');
  // ...and a malformed override fails closed.
  for (const bad of [{ maxObjects: NaN }, { maxObjects: -3 }, { maxObjects: 'x' }, { unknown: 1 }, [5]]) {
    g.limits = bad;
    assert.equal(authorizeWrite(f.state, f.auth, f.req).code, 'DENIED', JSON.stringify(bad));
  }
  // Request fields never carry limits.
  delete g.limits;
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, limits: { maxObjects: 99 }, objectCount: 21 }).code, 'LIMIT');
});

test('C1 cap changes apply immediately to existing grants: a lowered cap clamps, a raised cap lets valid overrides grow', () => {
  const f = fixture();
  const cloud = f.state.clients['client-A'];
  const g = f.grant('create_import', 'curated-A', 'client-A', 'memory', { limits: { maxObjects: 40 } });
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, objectCount: 35 }).code, 'ALLOWED');
  assert.equal(setOwnerCaps(f.state, OWNER, { maxObjects: 30 }).code, 'CAPS_SET');
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, objectCount: 35 }).code, 'LIMIT', 'cap decrease is enforced at once');
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, objectCount: 30 }).code, 'ALLOWED');
  assert.deepEqual(validateGrant(g, cloud, 1000, f.state), { ok: false, reason: 'limit-over-cap:maxObjects' }, 'the stored override is now over-cap');
  assert.equal(setOwnerCaps(f.state, OWNER, { maxObjects: DEFAULT_LIMITS.maxObjects - 1 }).code, 'INVALID', 'a cap cannot go below the safe default');
  assert.equal(setOwnerCaps(f.state, OWNER, { maxObjects: 100 }).code, 'CAPS_SET');
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, objectCount: 40 }).code, 'ALLOWED');
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, objectCount: 41 }).code, 'LIMIT', 'the grant override still bounds it');
});

test('C1 grant lifetime is bounded by the current owner cap and re-checked at enforcement', () => {
  const f = fixture();
  const cloud = f.state.clients['client-A'];
  const g = f.grant('create_import', 'curated-A', 'client-A', 'memory', { limits: { maxGrantMs: 45 * DAY }, expiresAt: 45 * DAY });
  assert.deepEqual(validateGrant(g, cloud, 1000, f.state), { ok: true });
  assert.deepEqual(validateGrant({ ...g, limits: { maxGrantMs: 61 * DAY }, expiresAt: 61 * DAY }, cloud, 1000, f.state), { ok: false, reason: 'limit-over-cap:maxGrantMs' });
  assert.deepEqual(validateGrant({ ...g, limits: undefined, expiresAt: 31 * DAY }, cloud, 1000, f.state), { ok: false, reason: 'cloud-expiry' }, 'default lifetime is 30 days');
  assert.equal(authorizeWrite(f.state, f.auth, f.req).code, 'ALLOWED');
  assert.equal(setOwnerCaps(f.state, OWNER, { maxGrantMs: 40 * DAY }).code, 'CAPS_SET');
  assert.equal(authorizeWrite(f.state, f.auth, f.req).code, 'DENIED', 'a 45-day grant outlives the lowered cap and stops working');
  assert.equal(setOwnerCaps(f.state, OWNER, { maxGrantMs: ENGINE_CEILINGS.maxGrantMs + 1 }).code, 'INVALID');
});

test('C1 the engine ceiling cannot be exceeded by any policy, grant or request', () => {
  const f = fixture();
  const g = f.grant('create_import', 'curated-A', 'client-A', 'memory', { limits: { maxBytes: ENGINE_CEILINGS.maxBytes } });
  assert.equal(setOwnerCaps(f.state, OWNER, { maxBytes: ENGINE_CEILINGS.maxBytes }).code, 'CAPS_SET');
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, payload: 'x'.repeat(ENGINE_CEILINGS.maxBytes) }).code, 'ALLOWED');
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, payload: 'x'.repeat(ENGINE_CEILINGS.maxBytes + 1) }).code, 'LIMIT');
  f.state.ownerPolicy.caps.maxBytes = 10 ** 9; // corrupted policy storage must not raise the ceiling
  g.limits.maxBytes = 10 ** 9;
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, payload: 'x'.repeat(ENGINE_CEILINGS.maxBytes + 1) }).code, 'LIMIT');
  f.state.ownerPolicy.caps.maxBytes = -1; // malformed storage falls back to the safe default, never to a higher value
  g.limits = undefined; delete g.limits;
  assert.equal(effectiveLimits(f.state, g).limits.maxBytes, DEFAULT_LIMITS.maxBytes);
});

test('C1 local dreaming concurrency: default 3, owner cap 5 (configurable), overrides never exceed the cap, enforced after policy updates', () => {
  const b = bulk();
  const jobs = (n) => Array.from({ length: n }, (_, i) => ({ ...b.job, jobId: 'w' + i }));
  assert.deepEqual([0, 1, 2].map((i) => b.start(jobs(3)[i]).code), ['STARTED', 'STARTED', 'STARTED']);
  assert.equal(b.start(jobs(4)[3]).code, 'LIMIT', 'the fourth worker exceeds the default of 3');
  b.state.now += DEFAULT_LIMITS.leaseMs; // leases expire and release their slots
  assert.equal(b.start(jobs(5)[3]).code, 'STARTED');

  const o = bulk({ stageExtra: { limits: { maxConcurrentWorkers: 5 } } });
  const five = Array.from({ length: 6 }, (_, i) => ({ ...o.job, jobId: 'w' + i }));
  assert.deepEqual(five.slice(0, 5).map((j) => o.start(j).code), Array(5).fill('STARTED'));
  assert.equal(o.start(five[5]).code, 'LIMIT', 'override 5 equals the owner cap of 5');
  const cloudless = o.state.clients['local-L'];
  assert.deepEqual(validateGrant({ ...o.grantOf('bulk_stage'), limits: { maxConcurrentWorkers: 6 } }, cloudless, 1000, o.state), { ok: false, reason: 'limit-over-cap:maxConcurrentWorkers' });
  o.grantOf('bulk_stage').limits.maxConcurrentWorkers = 6; // tampered record is clamped, not honored
  o.state.now += DEFAULT_LIMITS.leaseMs;
  assert.deepEqual(five.slice(0, 6).map((j, i) => o.start({ ...j, jobId: 'x' + i }).code), [...Array(5).fill('STARTED'), 'LIMIT']);

  const c = bulk({ stageExtra: { limits: { maxConcurrentWorkers: 8 } } });
  assert.equal(setOwnerCaps(c.state, OWNER, { maxConcurrentWorkers: ENGINE_CEILINGS.maxConcurrentWorkers + 1 }).code, 'INVALID');
  assert.equal(setOwnerCaps(c.state, OWNER, { maxConcurrentWorkers: 8 }).code, 'CAPS_SET');
  const eight = Array.from({ length: 9 }, (_, i) => ({ ...c.job, jobId: 'e' + i }));
  assert.deepEqual(eight.map((j) => c.start(j).code), [...Array(8).fill('STARTED'), 'LIMIT'], 'engine ceiling is 8');
  assert.equal(setOwnerCaps(c.state, OWNER, { maxConcurrentWorkers: 4 }).code, 'CAPS_SET');
  assert.equal(c.start({ ...c.job, jobId: 'late' }).code, 'LIMIT', 'cap decrease blocks new workers at once');
  assert.equal(stageBulkBatch(c.state, c.local, 'e0', 'bk', c.creates(1), { clusters: c.clusters }).code, 'STAGED', 'running workers keep working');
  assert.equal(setOwnerCaps(c.state, OWNER, { maxConcurrentWorkers: 2 }).code, 'INVALID', 'cap cannot go below the default of 3');
});

test('C1 bulk batch, item and byte limits follow the same default < cap < ceiling rules', () => {
  const b = bulk();
  b.start();
  assert.equal(b.stage('big', b.creates(DEFAULT_LIMITS.maxBatch + 1)).code, 'LIMIT');
  assert.equal(b.stage('ok', b.creates(DEFAULT_LIMITS.maxBatch)).code, 'STAGED');
  const bytes = ['A', 'B', 'C'].map((c, i) => ({ operation: 'create_import', payload: c.repeat(12_000), sources: ['raw-A'], evidence: [b.src('big' + i)], cluster: 'c1' }));
  assert.equal(b.stage('bytes', bytes).code, 'LIMIT', 'a staging request cannot exceed the payload limit');
  b.state.jobs['local-L:job-1'].itemCount = DEFAULT_LIMITS.maxItems - 1;
  assert.equal(b.stage('over', b.creates(2, 'z')).code, 'LIMIT');

  const o = bulk({ stageExtra: { limits: { maxBatch: 1024 } } });
  o.start();
  assert.equal(o.stage('big', o.creates(1025)).code, 'LIMIT');
  assert.equal(o.stage('ok', o.creates(1024)).code, 'STAGED', 'an override within the cap is honored');
  assert.deepEqual(validateGrant({ ...o.grantOf('bulk_stage'), limits: { maxBatch: FACTORY_OWNER_CAPS.maxBatch + 1 } }, o.state.clients['local-L'], 1000, o.state), { ok: false, reason: 'limit-over-cap:maxBatch' });
  assert.equal(setOwnerCaps(o.state, OWNER, { maxBatch: 600 }).code, 'CAPS_SET');
  assert.equal(o.stage('after', o.creates(601, 'q')).code, 'LIMIT', 'a lowered cap clamps the existing override');
  assert.equal(setOwnerCaps(o.state, OWNER, { maxBatch: ENGINE_CEILINGS.maxBatch }).code, 'CAPS_SET');
  assert.equal(setOwnerCaps(o.state, OWNER, { maxBatch: ENGINE_CEILINGS.maxBatch + 1 }).code, 'INVALID');
});


test('C1 the owner worker cap is vault-wide: several local clients cannot together exceed it', () => {
  const b = bulk();
  const m = { ...b.local, clientId: 'local-M' };
  b.grant('bulk_stage', b.target, 'local-M', 'memory');
  b.readGrant('raw-A', 'local-M');
  const mk = (id) => ({ ...b.job, jobId: id });
  assert.deepEqual(['l0', 'l1', 'l2'].map((id) => b.start(mk(id)).code), ['STARTED', 'STARTED', 'STARTED']);
  assert.deepEqual(['m0', 'm1'].map((id) => startBulkJob(b.state, m, mk(id)).code), ['STARTED', 'STARTED']);
  assert.equal(startBulkJob(b.state, m, mk('m2')).code, 'LIMIT', 'five workers already run in this vault (owner cap 5), though local-M itself runs only two');
  assert.equal(setOwnerCaps(b.state, OWNER, { maxConcurrentWorkers: 6 }).code, 'CAPS_SET');
  assert.equal(startBulkJob(b.state, m, mk('m2')).code, 'STARTED', 'a raised cap admits the next worker');
});

// ---------------- C2: worker lease != proposal lifetime != apply approval ----------------
test('C2 worker lease expiry does not destroy a valid staged proposal, and apply works after the worker expired', () => {
  const b = bulk();
  b.start(); b.stage();
  b.state.now += DEFAULT_LIMITS.leaseMs + 1; // the worker's execution lease is over
  assert.equal(b.stage('b2', b.creates(1, 'x')).code, 'DENIED', 'an expired worker cannot stage');
  assert.equal(b.state.batches['local-L:b1'].status, 'staged', 'the proposal survives');
  assert.equal(b.approve().code, 'APPROVED');
  assert.equal(b.apply().code, 'APPLIED', 'a reviewed proposal is applicable after its worker expired');
});

test('C2 the worker lease is renewable only while unexpired and fully authorized, and renewal does not extend the proposal', () => {
  const b = bulk();
  b.start();
  const staged = b.stage();
  const expiresAt = staged.expiresAt;
  b.state.now += DEFAULT_LIMITS.leaseMs - 1;
  const renewed = renewWorkerLease(b.state, b.local, 'job-1');
  assert.equal(renewed.code, 'RENEWED');
  assert.equal(renewed.leaseExpiresAt, b.state.now + DEFAULT_LIMITS.leaseMs);
  assert.equal(b.state.jobs['local-L:job-1'].renewals, 1);
  assert.equal(b.state.batches['local-L:b1'].expiresAt, expiresAt, 'renewal never extends a proposal');
  b.state.now += DEFAULT_LIMITS.leaseMs; // expires
  assert.equal(renewWorkerLease(b.state, b.local, 'job-1').code, 'DENIED', 'an expired lease cannot be revived');
  const c = bulk();
  c.start();
  c.state.readGrants.find((g) => g.clientId === 'local-L' && g.collectionId === 'raw-A').revoked = true;
  assert.equal(renewWorkerLease(c.state, c.local, 'job-1').code, 'DENIED', 'a revoked source blocks renewal');
  const d = bulk();
  d.start();
  d.grantOf('bulk_stage').revoked = true;
  assert.equal(renewWorkerLease(d.state, d.local, 'job-1').code, 'DENIED');
  assert.equal(renewWorkerLease(d.state, d.auth, 'job-1').code, 'DENIED', 'cloud credentials cannot renew');
});

test('C2 a staged proposal has its own TTL: an expired proposal is never approved or applied, whatever the worker lease', () => {
  const b = bulk();
  b.start();
  const staged = b.stage();
  assert.equal(staged.expiresAt, 1000 + DEFAULT_LIMITS.proposalTtlMs);
  b.approve();
  b.state.now = staged.expiresAt - 1;
  renewWorkerLease(b.state, b.local, 'job-1'); // the worker is alive, the proposal is not
  b.state.now = staged.expiresAt;
  const before = b.objects();
  assert.equal(b.apply().code, 'EXPIRED');
  assert.equal(b.state.batches['local-L:b1'].status, 'expired');
  assert.deepEqual(b.state.objects, before);
  assert.equal(b.apply().code, 'DENIED', 'an expired batch stays expired');
  const c = bulk();
  c.start(); const s2 = c.stage(); c.state.now = s2.expiresAt;
  assert.equal(ownerApproveBatch(c.state, OWNER, 'local-L', 'b1', { summaryDigest: 'x', decisions: {} }).code, 'EXPIRED', 'no approval for an expired proposal');
});

test('C2 proposal TTL is owner-configurable; tightening reaches staged proposals, loosening never resurrects them', () => {
  const b = bulk({ stageExtra: { limits: { proposalTtlMs: 10 * DAY } } });
  b.start();
  const staged = b.stage();
  assert.equal(staged.expiresAt, 1000 + 10 * DAY);
  assert.deepEqual(validateGrant({ ...b.grantOf('bulk_stage'), limits: { proposalTtlMs: 15 * DAY } }, b.state.clients['local-L'], 1000, b.state), { ok: false, reason: 'limit-over-cap:proposalTtlMs' });
  b.approve();
  assert.equal(setOwnerCaps(b.state, OWNER, { proposalTtlMs: 7 * DAY }).code, 'CAPS_SET'); // tighten to the default
  b.state.now = 1000 + 8 * DAY;
  assert.equal(b.apply().code, 'EXPIRED', 'the lowered cap applies to the already staged proposal');
  const c = bulk({ stageExtra: { limits: { proposalTtlMs: 7 * DAY } } });
  c.start(); const s = c.stage();
  setOwnerCaps(c.state, OWNER, { proposalTtlMs: 30 * DAY });
  c.state.grants.find((g) => g.operation === 'bulk_stage').limits.proposalTtlMs = 30 * DAY;
  c.state.now = s.expiresAt + 1;
  assert.equal(c.approve().code, 'EXPIRED', 'loosening the policy cannot extend an existing proposal');
});

test('C2 the owner approval is short-lived and single-use, independent of proposal and worker lifetimes', () => {
  const b = bulk();
  b.start(); b.stage(); b.approve();
  b.state.now += DEFAULT_LIMITS.approvalTtlMs + 1;
  assert.equal(b.apply().code, 'APPROVAL_REQUIRED', 'the approval expired long before the proposal');
  assert.equal(b.approve().code, 'APPROVED');
  assert.equal(b.apply().code, 'APPLIED');
  assert.equal(b.state.approvals.filter((a) => a.used).length, 1);
  assert.deepEqual(validateGrant({ ...b.grantOf('bulk_apply'), limits: { approvalTtlMs: FACTORY_OWNER_CAPS.approvalTtlMs + 1 } }, b.state.clients['local-L'], 1000, b.state), { ok: false, reason: 'limit-over-cap:approvalTtlMs' });
  const c = bulk({ applyExtra: { limits: { approvalTtlMs: 60 * 1000 } } });
  c.start(); c.stage();
  assert.equal(c.approve().expiresAt, 1000 + 60 * 1000, 'an owner-set approval TTL is honored within the cap');
});

test('C2 apply revalidates evidence authority: redaction, stale digests and removed sources, before and inside the commit', () => {
  const cases = {
    'evidence pointer redacted': [(s) => { s.privacy.redacted.push('raw-A#msg-1'); }, 'DENIED'],
    'whole source collection forgotten': [(s) => { s.privacy.redacted.push('raw-A#*'); }, 'DENIED'],
    'source content changed (stale digest)': [(s) => { s.sources['raw-A#msg-2'].digest = 'changed'; }, 'STALE_EVIDENCE'],
    'source item removed': [(s) => { delete s.sources['raw-A#msg-2']; }, 'STALE_EVIDENCE']
  };
  for (const [label, [mutate, code]] of Object.entries(cases)) {
    for (const when of ['before apply', 'inside commit']) {
      const b = bulk();
      b.start(); b.stage(); b.approve();
      const expected = b.objects();
      const res = when === 'before apply' ? (mutate(b.state), b.apply()) : b.apply('b1', { beforeCommit: mutate });
      assert.equal(res.code, code, label + ' / ' + when);
      assert.deepEqual(b.state.objects, expected, label + ' / ' + when + ': nothing applied');
      assert.equal(b.state.batches['local-L:b1'].status, 'staged');
    }
  }
  const b = bulk();
  b.start();
  b.state.privacy.redacted.push('raw-A#msg-1');
  assert.equal(b.stage().code, 'DENIED', 'a forgotten source cannot even be staged');
  b.state.privacy.redacted.length = 0;
  const stale = b.items(); stale[0].evidence[0].digest = 'not-current';
  assert.equal(b.stage('b2', stale).code, 'STALE_EVIDENCE', 'evidence must match the current source content at staging');
});

test('C2 jobs and staged proposals do not survive an unlock-epoch change, even for a freshly issued credential', () => {
  const b = bulk();
  b.start(); b.stage(); b.approve();
  b.state.epoch += 1;
  const fresh = { ...b.local, epoch: b.state.epoch };
  const expected = b.objects();
  assert.equal(applyBulkBatch(b.state, fresh, 'job-1', 'b1').code, 'DENIED');
  assert.equal(stageBulkBatch(b.state, fresh, 'job-1', 'b2', b.creates(1, 'e'), { clusters: b.clusters }).code, 'DENIED');
  assert.equal(renewWorkerLease(b.state, fresh, 'job-1').code, 'DENIED');
  assert.deepEqual(b.state.objects, expected);
  // the same holds if the credential is re-issued in the middle of the commit
  const c = bulk();
  c.start(); c.stage(); c.approve();
  const before = c.objects();
  assert.equal(c.apply('b1', { beforeCommit(s) { s.epoch += 1; c.local.epoch = s.epoch; } }).code, 'DENIED');
  assert.deepEqual(c.state.objects, before);
});

test('C2 apply revalidates epoch, tombstone state and expected revisions', () => {
  const cases = {
    'epoch change': [(s) => { s.epoch += 1; }, 'DENIED'],
    'target tombstoned after staging': [(s) => { s.objects.m1.tombstoned = true; }, 'DENIED'],
    'target moved to another collection': [(s) => { s.objects.m1.collectionId = 'curated-B'; }, 'DENIED'],
    'target revised after staging': [(s) => { s.objects.m1.revision = 5; }, 'CONFLICT']
  };
  for (const [label, [mutate, code]] of Object.entries(cases)) {
    for (const when of ['before apply', 'inside commit']) {
      const b = bulk();
      b.start(); b.stage(); b.approve();
      const res = when === 'before apply' ? (mutate(b.state), b.apply()) : b.apply('b1', { beforeCommit: mutate });
      assert.equal(res.code, code, label + ' / ' + when);
      assert.equal(b.state.objects['obj:local-L:b1:0'], undefined, label + ' / ' + when + ': nothing applied');
    }
  }
});

// ---------------- C3: review by exception ----------------
function exceptionBatch(b) {
  b.start();
  b.state.objects.m1.origin = 'owner'; // an owner-authored claim
  const items = [
    ...b.creates(3, 'a').map((i) => ({ ...i, cluster: 'c1' })),
    { operation: 'update', objectId: 'm1', expectedRevision: 2, payload: 'rewritten by agent', sources: ['raw-A'], evidence: [b.src('msg-9'), b.src('msg-8')], cluster: 'c2' }
  ];
  return b.stage('b1', items, { clusters: [{ id: 'c1', theme: 'Architecture notes' }, { id: 'c2', theme: 'Decisions' }] });
}

test('C3 the approval surface exposes summary, counts, risk breakdown, exceptions, and expandable evidence and diff', () => {
  const b = bulk();
  assert.equal(exceptionBatch(b).code, 'STAGED');
  const s = b.surface();
  assert.deepEqual([s.totalItems, s.operations, s.risk, s.consolidatedItems, s.evidenceCount], [4, { create_import: 3, update: 1 }, { low: 3, medium: 0, high: 1 }, 1, 5]);
  assert.deepEqual(s.clusters.map((c) => [c.id, c.theme, c.themeIsUntrustedText, c.items]), [['c1', 'Architecture notes', true, 3], ['c2', 'Decisions', true, 1]]);
  assert.deepEqual(s.exceptions, [{ index: 3, flags: ['owner_claim'], operation: 'update', objectId: 'm1', cluster: 'c2', evidenceCount: 2 }]);
  assert.deepEqual(s.requiredDecisions, [3]);
  assert.deepEqual(b.surface(), s, 'the surface is deterministic');
  assert.equal(JSON.stringify(s).includes('rewritten by agent'), false, 'the concise summary carries no item content');
  const detail = reviewItemDetail(b.state, 'local-L', 'b1', 3);
  assert.deepEqual([detail.before, detail.after, detail.risk, detail.evidence.length], ['old', 'rewritten by agent', 'high', 2]);
  assert.deepEqual(reviewFullDiff(b.state, 'local-L', 'b1').map((d) => d.index), [0, 1, 2, 3]);
  assert.equal(reviewItemDetail(b.state, 'local-L', 'b1', 99), null);
});

test('C3 risk is deterministic: model confidence, scores and labels are ignored and never digest-bound', () => {
  const b = bulk();
  b.start();
  b.state.objects.m1.origin = 'owner';
  const risky = (extra) => [{ operation: 'update', objectId: 'm1', expectedRevision: 2, payload: 'x', sources: ['raw-A'], evidence: [b.src('m')], cluster: 'c1', ...extra }];
  const a = b.stage('ba', risky({ confidence: 0.999, risk: 'low', label: 'safe', autoApply: true }));
  const staged = b.state.batches['local-L:ba'].items[0];
  assert.deepEqual(Object.keys(staged).sort(), ['cluster', 'evidence', 'expectedRevision', 'objectId', 'operation', 'payload', 'sources', 'subject']);
  assert.equal(b.surface('ba').risk.high, 1, 'an owner-authored claim is exceptional whatever the model says');
  const clean = bulk(); clean.start(); clean.state.objects.m1.origin = 'owner';
  const same = clean.stage('ba', [{ operation: 'update', objectId: 'm1', expectedRevision: 2, payload: 'x', sources: ['raw-A'], evidence: [clean.src('m')], cluster: 'c1' }]);
  assert.equal(a.batchDigest, same.batchDigest, 'confidence/label fields do not change the digest');
});

test('C3 deterministic classification rules: low, medium, and the high-risk exception classes', () => {
  const kinds = (b, batch, its, clusters) => (b.stage(batch, its, { clusters: clusters ?? b.clusters }), b.surface(batch));
  const b = bulk(); b.start();
  const s1 = kinds(b, 'b1', [...b.creates(1, 'p'), { operation: 'append', objectId: 'm1', expectedRevision: 2, payload: 'extra', sources: ['raw-A'], evidence: [b.src('a')], cluster: 'c1' }]);
  assert.deepEqual(s1.risk, { low: 1, medium: 1, high: 0 });
  const s = bulk(); s.start(); s.state.collections['shared-C'].sensitive = true;
  assert.deepEqual(kinds(s, 'b1', s.creates(1, 'q')).exceptions[0].flags, ['sensitive_target']);
  const p = bulk({ target: 'curated-owner', scope: ['raw-private'] }); p.start();
  assert.deepEqual(kinds(p, 'b1', [{ ...p.creates(1)[0], sources: ['raw-private'], evidence: [p.src('pv', 'raw-private')] }]).exceptions[0].flags, ['private_source']);
  const c = bulk(); c.start();
  const conflicting = [
    { ...c.creates(1, 'k')[0], subject: 'owner-timezone' },
    { ...c.creates(1, 'l')[0], subject: 'owner-timezone', payload: 'a different claim', evidence: [c.src('l2')] }
  ];
  const sc = kinds(c, 'b1', conflicting);
  assert.deepEqual(sc.exceptions.map((e) => e.flags), [['conflict'], ['conflict']], 'contradictory items in one batch are both exceptions');
});

test('C3 exceptions need an explicit owner decision: confirm applies the item, withhold excludes it, undecided blocks', () => {
  const run = (decision) => {
    const b = bulk();
    exceptionBatch(b);
    return { b, approve: b.approve('b1', decision) };
  };
  assert.equal(run({}).approve.code, 'INVALID', 'every exception must be decided');
  assert.equal(run({ 0: 'confirm', 3: 'confirm' }).approve.code, 'INVALID', 'only exceptions can be decided');
  assert.equal(run({ 3: 'maybe' }).approve.code, 'INVALID');
  const confirmed = run({ 3: 'confirm' });
  assert.equal(confirmed.approve.code, 'APPROVED');
  const ok = confirmed.b.apply();
  assert.deepEqual([ok.code, ok.created, ok.changed, ok.withheld], ['APPLIED', 3, 1, 0]);
  assert.equal(confirmed.b.state.objects.m1.data, 'rewritten by agent');
  const withheld = run({ 3: 'withhold' });
  const w = (withheld.approve, withheld.b.apply());
  assert.deepEqual([w.code, w.created, w.changed, w.withheld], ['APPLIED', 3, 0, 1]);
  assert.equal(withheld.b.state.objects.m1.data, 'old', 'a withheld exception is never applied');
  assert.deepEqual(withheld.b.state.batches['local-L:b1'].applied.withheld, [3]);
  // a forged approval without per-exception decisions does not unlock the batch
  const forged = bulk(); exceptionBatch(forged);
  const sur = forged.surface();
  forged.state.approvals.push({ ownerApproved: true, clientId: 'local-L', vaultId: 'vault-A', collectionId: 'shared-C', operation: 'bulk_apply', operationId: 'b1',
    bodyDigest: sur.batchDigest, summaryDigest: sur.summaryDigest, decisions: {}, decisionsDigest: 'x', riskPolicyVersion: RISK_POLICY_VERSION, policyRevision: 4, epoch: 17, expiresAt: 9e9 });
  assert.equal(forged.apply().code, 'APPROVAL_REQUIRED');
  // a withheld item whose evidence was later forgotten does not block the rest
  const redacted = run({ 3: 'withhold' });
  redacted.b.state.privacy.redacted.push('raw-A#msg-9');
  assert.equal(redacted.b.apply().code, 'APPLIED');
});

test('C3 the approval is bound to the exact immutable displayed batch: any mutation or stale view invalidates it', () => {
  const b = bulk();
  b.start(); b.stage();
  const shown = b.surface();
  assert.equal(ownerApproveBatch(b.state, OWNER, 'local-L', 'b1', { summaryDigest: 'f'.repeat(64), decisions: {} }).code, 'STALE_SUMMARY');
  assert.equal(ownerApproveBatch(b.state, { localUi: false, authenticated: true }, 'local-L', 'b1', { summaryDigest: shown.summaryDigest, decisions: {} }).code, 'DENIED');
  assert.equal(b.approve('b1', {}, shown.summaryDigest).code, 'APPROVED');
  const base = structuredClone(b.state);
  const mutations = {
    'payload changed': (s) => { s.batches['local-L:b1'].items[0].payload = 'tampered'; },
    'item added': (s) => { const x = s.batches['local-L:b1']; x.items.push({ ...x.items[0], payload: 'extra' }); },
    'item removed': (s) => { s.batches['local-L:b1'].items.pop(); },
    'cluster theme rewritten': (s) => { s.batches['local-L:b1'].clusters[0].theme = 'Something harmless'; },
    'target object became owner-authored (risk changes)': (s) => { s.objects.m1.origin = 'owner'; },
    'risk policy version differs': (s) => { s.approvals.at(-1).riskPolicyVersion = RISK_POLICY_VERSION + 1; },
    'decisions tampered after approval': (s) => { s.approvals.at(-1).decisions = { 0: 'confirm' }; },
    'approval belongs to another batch': (s) => { s.approvals.at(-1).operationId = 'other'; },
    'approval summary digest replaced': (s) => { s.approvals.at(-1).summaryDigest = 'f'.repeat(64); }
  };
  for (const [label, mutate] of Object.entries(mutations)) {
    const s = structuredClone(base);
    mutate(s);
    const expected = structuredClone(s.objects);
    const res = applyBulkBatch(s, b.local, 'job-1', 'b1');
    assert.equal(res.ok, false, label);
    assert.notEqual(res.code, 'APPLIED', label);
    assert.deepEqual(s.objects, expected, label + ': nothing applied');
    assert.equal(s.approvals.every((a) => a.used !== true), true, label + ': approval not consumed by a failed apply');
  }
  // the owner's approval for this batch still works on the untouched state
  assert.equal(applyBulkBatch(structuredClone(base), b.local, 'job-1', 'b1').code, 'APPLIED');
  // mid-commit mutation of the immutable batch is caught by the second verification
  const mid = structuredClone(base);
  assert.equal(applyBulkBatch(mid, b.local, 'job-1', 'b1', { beforeCommit(s) { s.batches['local-L:b1'].items[0].payload = 'swapped'; } }).ok, false);
  assert.equal(mid.objects['obj:local-L:b1:0'], undefined);
});

test('C3 each approval binding holds on its own: batch digest, decisions digest and decision coverage', () => {
  const approved = () => { const b = bulk(); exceptionBatch(b); assert.equal(b.approve('b1', { 3: 'confirm' }).code, 'APPROVED'); return b; };
  const tamper = (mutate) => { const b = approved(); mutate(b.state.approvals.at(-1)); return b.apply().code; };
  assert.equal(tamper((a) => { a.bodyDigest = 'f'.repeat(64); }), 'APPROVAL_REQUIRED', 'batch digest of the approval');
  assert.equal(tamper((a) => { a.decisions = { 3: 'withhold' }; }), 'APPROVAL_REQUIRED', 'decisions cannot be changed after the owner decided');
  const emptyDecisionsDigest = createHash('sha256').update('{}').digest('hex');
  assert.equal(tamper((a) => { a.decisions = {}; a.decisionsDigest = emptyDecisionsDigest; }), 'APPROVAL_REQUIRED', 'an approval that omits the exception decision never applies');
  assert.equal(tamper((a) => { a.decisions = { 0: 'confirm', 3: 'confirm' }; a.decisionsDigest = createHash('sha256').update('{"0":"confirm","3":"confirm"}').digest('hex'); }), 'APPROVAL_REQUIRED', 'extra decisions are not accepted');
  assert.equal(approved().apply().code, 'APPLIED', 'the untouched approval works');
});

test('C3 a batch mutated after staging is rejected even if the owner is later shown and approves the mutated content', () => {
  const b = bulk();
  b.start(); b.stage();
  b.state.batches['local-L:b1'].items[0].payload = 'swapped after staging';
  assert.equal(b.approve().code, 'APPROVED', 'the approval UI only reflects the stored batch');
  const before = b.objects();
  assert.equal(b.apply().code, 'DENIED', 'the staged batch is immutable: its recomputed digest must equal the digest recorded at staging');
  assert.deepEqual(b.state.objects, before);
});

test('C3 low-risk auto-apply exists only under explicit owner policy and deterministic checks', () => {
  const owner = bulk({ applyMode: 'allow' });
  owner.start();
  owner.stage('b1', owner.creates(2));
  // default policy: disabled, so even a trivially low-risk batch with every grant `allow` and a confident model needs the owner
  assert.equal(owner.surface().autoApplyEligible, false);
  assert.equal(owner.apply().code, 'APPROVAL_REQUIRED');
  // policy can only be written by the owner, only for low risk, only for local clients, within the cap
  assert.equal(setAutoApplyPolicy(owner.state, { localUi: false, authenticated: true }, { enabled: true, maxRisk: 'low', maxItems: 10, clients: ['local-L'] }).code, 'DENIED');
  for (const bad of [{ maxRisk: 'medium' }, { maxRisk: 'high' }, { maxItems: 0 }, { maxItems: -1 }, { maxItems: FACTORY_OWNER_CAPS.maxBatch + 1 }, { clients: ['client-A'] }, { clients: ['ghost'] }, { enabled: 'yes' }]) {
    assert.equal(setAutoApplyPolicy(owner.state, OWNER, { enabled: true, maxRisk: 'low', maxItems: 10, clients: ['local-L'], ...bad }).code, 'INVALID', JSON.stringify(bad));
  }
  assert.equal(owner.autoApply({ clients: [] }).code, 'AUTO_APPLY_SET');
  assert.equal(owner.apply().code, 'APPROVAL_REQUIRED', 'a client that is not listed is never auto-applied');
  assert.equal(owner.autoApply({ maxItems: 1 }).code, 'AUTO_APPLY_SET');
  assert.equal(owner.apply().code, 'APPROVAL_REQUIRED', 'a batch above the policy item limit needs the owner');
  assert.equal(owner.autoApply().code, 'AUTO_APPLY_SET');
  const used = owner.apply();
  assert.deepEqual([used.code, used.auto, used.created], ['APPLIED', true, 2], 'eligible low-risk batch applies without an approval record');
  assert.equal(owner.state.approvals.length, 0);
  assert.equal(owner.state.audit.at(-1).code, 'APPLIED');
});

test('C3 auto-apply is denied for anything above low risk, ask_first, withheld policy changes and stale conditions', () => {
  const mixed = bulk({ applyMode: 'allow' });
  mixed.autoApply();
  mixed.start(); mixed.stage('b1', mixed.items()); // contains an update: medium
  assert.equal(mixed.surface().autoApplyEligible, false);
  assert.equal(mixed.apply().code, 'APPROVAL_REQUIRED', 'medium risk always needs the owner');
  const ask = bulk({ applyMode: 'ask_first' });
  ask.autoApply(); ask.start(); ask.stage('b1', ask.creates(1));
  assert.equal(ask.apply().code, 'APPROVAL_REQUIRED', 'bulk_apply=ask_first outranks the auto-apply policy');
  const sens = bulk({ applyMode: 'allow' });
  sens.autoApply(); sens.start(); sens.state.collections['shared-C'].sensitive = true; sens.stage('b1', sens.creates(1));
  assert.equal(sens.apply().code, 'APPROVAL_REQUIRED', 'sensitive targets are exceptions, never auto-applied');
  const flip = bulk({ applyMode: 'allow' });
  flip.autoApply(); flip.start(); flip.stage('b1', flip.creates(2));
  const expected = flip.objects();
  assert.equal(flip.apply('b1', { beforeCommit(s) { s.ownerPolicy.autoApply.enabled = false; } }).code, 'APPROVAL_REQUIRED', 'disabling the policy before commit stops it');
  assert.deepEqual(flip.state.objects, expected);
  const stale = bulk({ applyMode: 'allow' });
  stale.autoApply(); stale.start(); stale.stage('b1', stale.creates(2));
  assert.equal(stale.apply('b1', { beforeCommit(s) { s.privacy.redacted.push('raw-A#n' + 'r0'); } }).code, 'DENIED', 'redaction still blocks an auto-apply');
  const exp = bulk({ applyMode: 'allow' });
  exp.autoApply(); exp.start(); const s = exp.stage('b1', exp.creates(2)); exp.state.now = s.expiresAt;
  assert.equal(exp.apply().code, 'EXPIRED', 'auto-apply never applies an expired proposal');
});

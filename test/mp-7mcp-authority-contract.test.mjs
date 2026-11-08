// TEST-ONLY MP-7MCP-01 contract tests. No product endpoint is enabled by these fixtures.
// They exercise the isolated reference model (test/support), not production MCP authorization.
import test from 'node:test';
import assert from 'node:assert/strict';
import {
  applyBulkBatch, authorizeWrite, BULK_DEFAULTS, canRead, CLOUD_DEFAULTS, digestOf,
  ownerRestoreTombstone, ownerRollbackBatch, simulateWrite, stageBulkBatch, startBulkJob, validateGrant
} from './support/mp-7mcp-authority-model.mjs';

const OWNER = Object.freeze({ localUi: true, authenticated: true });

function fixture(operation = 'create_import', kind = 'memory') {
  const state = {
    vaultId: 'vault-A', audience: 'mind-place-mcp', epoch: 17, now: 1000,
    locked: false, paused: false, authorityReadable: true,
    receipts: new Map(), audit: [], writeLog: [], approvals: [], grants: [], readGrants: [], jobs: {}, batches: {},
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
      'local-L': { enabled: true, tier: 'local', writePolicyRevision: 4, readPolicyRevision: 2 }
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
      expiresAt: client === 'local-L' ? null : 3000, ...extra
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
  assert.deepEqual(validateGrant({ ...f.state.grants[0] }, local, 1000), { ok: false, reason: 'ineligible' });
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

test('F7 cloud limits: 1 MiB, 20 objects, 60 writes per hour; the owner can configure them', () => {
  const f = fixture();
  const g = f.grant();
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, payload: 'x'.repeat(CLOUD_DEFAULTS.maxBytes + 1) }).code, 'LIMIT');
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, objectCount: CLOUD_DEFAULTS.maxObjects + 1 }).code, 'LIMIT');
  g.limits = { maxBytes: 2_000_000 };
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, payload: 'x'.repeat(CLOUD_DEFAULTS.maxBytes + 1) }).code, 'ALLOWED');
  g.limits = { maxWritesPerHour: 2 };
  assert.equal(simulateWrite(f.state, f.auth, { ...f.req, operationId: 'w1' }).code, 'COMMITTED');
  assert.equal(simulateWrite(f.state, f.auth, { ...f.req, operationId: 'w2' }).code, 'COMMITTED');
  assert.equal(simulateWrite(f.state, f.auth, { ...f.req, operationId: 'w3' }).code, 'LIMIT');
  f.state.now += 3_600_001; g.expiresAt = f.state.now + 1000;
  assert.equal(simulateWrite(f.state, f.auth, { ...f.req, operationId: 'w4' }).code, 'COMMITTED');
});

test('F7 grant issuance rejects wildcards, empty values, unknown modes and cloud bulk/raw/long expiry', () => {
  const f = fixture();
  const cloud = f.state.clients['client-A'];
  const good = f.grant();
  assert.deepEqual(validateGrant(good, cloud, 1000), { ok: true });
  for (const [patch, reason] of [
    [{ collectionId: '*' }, 'field:collectionId'], [{ collectionId: 'curated-' + '*' }, 'field:collectionId'],
    [{ collectionId: '' }, 'field:collectionId'], [{ clientId: 'a b' }, 'field:clientId'],
    [{ operation: 'grant_admin' }, 'operation'], [{ mode: 'always' }, 'mode'], [{ objectKind: 'unknown' }, 'kind'],
    [{ operation: 'bulk_stage' }, 'cloud-bulk'], [{ objectKind: 'graph_batch' }, 'cloud-layer'],
    [{ expiresAt: 1000 + CLOUD_DEFAULTS.maxGrantMs + 1 }, 'cloud-expiry'], [{ expiresAt: null }, 'cloud-expiry']
  ]) {
    assert.deepEqual(validateGrant({ ...good, ...patch }, cloud, 1000), { ok: false, reason }, JSON.stringify(patch));
  }
  assert.deepEqual(validateGrant({ ...good, expiresAt: 1000 + CLOUD_DEFAULTS.maxGrantMs }, cloud, 1000), { ok: true });
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

// ---- bulk (local tier) ----
function bulk(opts = {}) {
  const f = fixture('update');
  const target = opts.target ?? 'shared-C';
  f.grant('bulk_stage', target, 'local-L', 'memory', opts.stageExtra);
  f.grant('bulk_apply', target, 'local-L', 'memory', { mode: opts.applyMode ?? 'ask_first' });
  for (const op of ['create_import', 'append', 'update']) f.grant(op, target, 'local-L', 'memory');
  f.readGrant('raw-A', 'local-L'); f.readGrant('curated-A', 'local-L');
  f.state.objects.m1 = { vaultId: 'vault-A', collectionId: target, objectKind: 'memory', revision: 2, data: 'old', history: [] };
  const job = { jobId: 'job-1', targetCollectionId: target, objectKind: 'memory', scopeCollections: ['raw-A'] };
  const items = (extra = {}) => [
    { operation: 'create_import', payload: 'new curated note', sources: ['raw-A'], evidence: [{ collectionId: 'raw-A', ref: 'msg-1' }], ...extra },
    { operation: 'update', objectId: 'm1', expectedRevision: 2, payload: 'merged', sources: ['raw-A'], evidence: [{ collectionId: 'raw-A', ref: 'msg-2' }] }
  ];
  const approveBatch = (batchId = 'b1', digest) => f.state.approvals.push({
    ownerApproved: true, clientId: 'local-L', vaultId: 'vault-A', collectionId: target, operation: 'bulk_apply',
    operationId: batchId, bodyDigest: digest, policyRevision: 4, epoch: 17, expiresAt: 3000
  });
  return { ...f, job, items, approveBatch, target };
}

test('bulk: cloud clients cannot start bulk jobs or smuggle bulk through single writes', () => {
  const b = bulk();
  b.grant('bulk_stage', 'curated-A', 'client-A');
  assert.equal(startBulkJob(b.state, b.auth, { ...b.job, targetCollectionId: 'curated-A' }).code, 'DENIED');
  b.grant('create_import', 'curated-A', 'client-A');
  assert.equal(authorizeWrite(b.state, b.auth, { ...b.req, operation: 'create_import', bulk: true }).code, 'DENIED');
  assert.equal(startBulkJob(b.state, { ...b.local, transport: 'tunnel' }, b.job).code, 'DENIED');
});

test('bulk: staged proposals are invisible until a separate apply; evidence and scope are mandatory', () => {
  const b = bulk();
  assert.equal(startBulkJob(b.state, b.local, b.job).code, 'STARTED');
  const before = JSON.stringify(b.state.objects);
  const noEvidence = b.items().map((i) => ({ ...i, evidence: [] }));
  assert.equal(stageBulkBatch(b.state, b.local, 'job-1', 'bx', noEvidence).code, 'DENIED');
  assert.equal(stageBulkBatch(b.state, b.local, 'job-1', 'bx', b.items({ sources: ['curated-A'] })).code, 'DENIED', 'sources must stay inside the declared scope');
  const staged = stageBulkBatch(b.state, b.local, 'job-1', 'b1', b.items());
  assert.equal(staged.code, 'STAGED');
  assert.equal(JSON.stringify(b.state.objects), before, 'nothing is visible before apply');
  assert.equal(b.state.batches['local-L:b1'].evidenceCount, 2);
});

test('bulk: apply defaults to ask_first, is single-use and bound to the exact batch digest', () => {
  const b = bulk();
  startBulkJob(b.state, b.local, b.job);
  const staged = stageBulkBatch(b.state, b.local, 'job-1', 'b1', b.items());
  assert.equal(applyBulkBatch(b.state, b.local, 'job-1', 'b1').code, 'APPROVAL_REQUIRED');
  b.approveBatch('b1', 'f'.repeat(64));
  assert.equal(applyBulkBatch(b.state, b.local, 'job-1', 'b1').code, 'APPROVAL_REQUIRED', 'approval for another digest is useless');
  b.approveBatch('b1', staged.batchDigest);
  const applied = applyBulkBatch(b.state, b.local, 'job-1', 'b1');
  assert.deepEqual([applied.code, applied.created, applied.changed], ['APPLIED', 1, 1]);
  assert.equal(b.state.objects.m1.data, 'merged');
  assert.equal(b.state.objects.m1.revision, 3);
  assert.equal(applyBulkBatch(b.state, b.local, 'job-1', 'b1').code, 'DENIED', 'a batch applies once');
  assert.equal(b.state.approvals.every((a) => a.bodyDigest !== staged.batchDigest || a.used === true), true);
});

test('bulk: an owner who sets apply to allow removes the per-batch approval', () => {
  const b = bulk({ applyMode: 'allow' });
  startBulkJob(b.state, b.local, b.job);
  stageBulkBatch(b.state, b.local, 'job-1', 'b1', b.items());
  assert.equal(applyBulkBatch(b.state, b.local, 'job-1', 'b1').code, 'APPLIED');
});

test('bulk: limits default to 15 min, 512 per batch, 20,000 per job and are owner-configurable', () => {
  const b = bulk();
  assert.deepEqual(BULK_DEFAULTS, { leaseMs: 900_000, maxBatch: 512, maxItems: 20_000 });
  startBulkJob(b.state, b.local, b.job);
  const many = (n) => Array.from({ length: n }, (_, i) => ({ operation: 'create_import', payload: 'n' + i, sources: ['raw-A'], evidence: [{ collectionId: 'raw-A', ref: 'r' + i }] }));
  assert.equal(stageBulkBatch(b.state, b.local, 'job-1', 'big', many(513)).code, 'LIMIT');
  assert.equal(stageBulkBatch(b.state, b.local, 'job-1', 'ok', many(512)).code, 'STAGED');
  b.state.jobs['local-L:job-1'].itemCount = 20_000 - 1;
  assert.equal(stageBulkBatch(b.state, b.local, 'job-1', 'over', many(2)).code, 'LIMIT');
  b.state.now += 900_000;
  assert.equal(stageBulkBatch(b.state, b.local, 'job-1', 'late', many(1)).code, 'DENIED', 'lease expired');
  const c = bulk({ stageExtra: { limits: { leaseMs: 60_000, maxBatch: 2, maxItems: 5 } } });
  startBulkJob(c.state, c.local, c.job);
  assert.equal(stageBulkBatch(c.state, c.local, 'job-1', 'three', many(3)).code, 'LIMIT');
});

test('bulk scope rule: private sources never feed a broader audience; ordinary raw can be promoted', () => {
  const b = bulk();
  b.readGrant('raw-private', 'local-L');
  assert.equal(startBulkJob(b.state, b.local, { ...b.job, scopeCollections: ['raw-private'] }).code, 'DENIED');
  assert.equal(startBulkJob(b.state, b.local, { ...b.job, scopeCollections: ['raw-A'], targetCollectionId: 'shared-C' }).code, 'STARTED');
  const o = bulk({ target: 'curated-owner' });
  o.readGrant('raw-private', 'local-L');
  assert.equal(startBulkJob(o.state, o.local, { ...o.job, scopeCollections: ['raw-private'] }).code, 'STARTED', 'owner-only private source into an owner-only target');
  const c = bulk({ target: 'graph-A' });
  assert.equal(startBulkJob(c.state, c.local, { ...c.job, objectKind: 'memory', targetCollectionId: 'graph-A' }).code, 'DENIED', 'kind must match the collection layer');
});

test('bulk apply is atomic: one stale item applies nothing', () => {
  const b = bulk({ applyMode: 'allow' });
  startBulkJob(b.state, b.local, b.job);
  stageBulkBatch(b.state, b.local, 'job-1', 'b1', b.items());
  const expected = structuredClone(b.state.objects);
  expected.m1.revision = 3; // the only change: the concurrent edit injected below
  const res = applyBulkBatch(b.state, b.local, 'job-1', 'b1', { beforeCommit(s) { s.objects.m1.revision = 3; } });
  assert.equal(res.code, 'CONFLICT');
  assert.deepEqual(b.state.objects, expected, 'no item of the batch was applied');
  assert.equal(b.state.batches['local-L:b1'].status, 'staged');
});

test('bulk: revoke, Lock or epoch change between stage and apply denies the whole batch', () => {
  for (const mutate of [
    (s) => { s.epoch += 1; }, (s) => { s.locked = true; }, (s) => { s.clients['local-L'].writePolicyRevision += 1; },
    (s) => { s.grants.find((g) => g.operation === 'bulk_apply').revoked = true; }
  ]) {
    const b = bulk({ applyMode: 'allow' });
    startBulkJob(b.state, b.local, b.job);
    stageBulkBatch(b.state, b.local, 'job-1', 'b1', b.items());
    mutate(b.state);
    assert.equal(applyBulkBatch(b.state, b.local, 'job-1', 'b1').code, 'DENIED');
    assert.equal(b.state.objects.m1.revision, 2);
  }
});

test('bulk: tombstone and move are never bulk operations', () => {
  const b = bulk({ applyMode: 'allow' });
  b.grant('delete', b.target, 'local-L', 'memory'); b.grant('move', b.target, 'local-L', 'memory');
  startBulkJob(b.state, b.local, b.job);
  for (const operation of ['delete', 'move']) {
    const items = [{ operation, objectId: 'm1', expectedRevision: 2, payload: '', sources: ['raw-A'], evidence: [{ collectionId: 'raw-A', ref: 'r' }] }];
    assert.equal(stageBulkBatch(b.state, b.local, 'job-1', 'b-' + operation, items).code, 'DENIED', operation);
  }
});

test('bulk rollback is owner-only and logical: created objects are tombstoned, changed objects get a new revision', () => {
  const b = bulk({ applyMode: 'allow' });
  startBulkJob(b.state, b.local, b.job);
  stageBulkBatch(b.state, b.local, 'job-1', 'b1', b.items());
  applyBulkBatch(b.state, b.local, 'job-1', 'b1');
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
  const b = bulk({ applyMode: 'allow' });
  startBulkJob(b.state, b.local, b.job);
  stageBulkBatch(b.state, b.local, 'job-1', 'b1', b.items());
  applyBulkBatch(b.state, b.local, 'job-1', 'b1');
  b.state.objects.m1 = { ...b.state.objects.m1, revision: 9, data: 'edited later' };
  assert.equal(ownerRollbackBatch(b.state, OWNER, 'local-L', 'b1').code, 'CONFLICT');
  assert.equal(b.state.objects.m1.data, 'edited later');
  assert.equal(b.state.objects['obj:local-L:b1:0'].tombstoned, false, 'all-or-nothing');
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
      const b = bulk({ applyMode: 'allow' });
      startBulkJob(b.state, b.local, b.job);
      stageBulkBatch(b.state, b.local, 'job-1', 'b1', b.items());
      const expected = structuredClone(b.state.objects);
      let res;
      if (when === 'before apply') { mutate(b.state); res = applyBulkBatch(b.state, b.local, 'job-1', 'b1'); }
      else res = applyBulkBatch(b.state, b.local, 'job-1', 'b1', { beforeCommit: mutate });
      assert.equal(res.code, 'DENIED', label + ' / ' + when);
      assert.deepEqual(b.state.objects, expected, label + ' / ' + when + ': nothing applied');
      assert.equal(b.state.batches['local-L:b1'].status, 'staged');
    }
  }
});

test('bulk apply never weakens a per-operation policy: the strictest mode wins', () => {
  const b = bulk({ applyMode: 'allow' });
  b.state.grants.find((g) => g.operation === 'update' && g.clientId === 'local-L').mode = 'ask_first';
  startBulkJob(b.state, b.local, b.job);
  const staged = stageBulkBatch(b.state, b.local, 'job-1', 'b1', b.items());
  assert.equal(applyBulkBatch(b.state, b.local, 'job-1', 'b1').code, 'APPROVAL_REQUIRED', 'bulk_apply=allow does not bypass update=ask_first');
  b.approveBatch('b1', 'f'.repeat(64));
  assert.equal(applyBulkBatch(b.state, b.local, 'job-1', 'b1').code, 'APPROVAL_REQUIRED', 'approval must be bound to this batch digest');
  b.approveBatch('b1', staged.batchDigest);
  assert.equal(applyBulkBatch(b.state, b.local, 'job-1', 'b1').code, 'APPLIED', 'one batch approval satisfies every ask_first operation inside it');
  assert.equal(b.state.approvals.at(-1).used, true, 'single-use');
});

test('bulk: ask_first on an operation absent from the batch does not demand approval; a mode flip before commit does', () => {
  const b = bulk({ applyMode: 'allow' });
  b.state.grants.find((g) => g.operation === 'append' && g.clientId === 'local-L').mode = 'ask_first';
  startBulkJob(b.state, b.local, b.job);
  stageBulkBatch(b.state, b.local, 'job-1', 'b1', b.items()); // create_import + update only
  assert.equal(applyBulkBatch(b.state, b.local, 'job-1', 'b1').code, 'APPLIED', 'append is not in this batch');
  const c = bulk({ applyMode: 'allow' });
  startBulkJob(c.state, c.local, c.job);
  stageBulkBatch(c.state, c.local, 'job-1', 'b1', c.items());
  const expected = structuredClone(c.state.objects);
  const res = applyBulkBatch(c.state, c.local, 'job-1', 'b1', {
    beforeCommit(s) { s.grants.find((g) => g.operation === 'update' && g.clientId === 'local-L').mode = 'ask_first'; }
  });
  assert.equal(res.code, 'APPROVAL_REQUIRED', 'policy tightened between stage and commit');
  assert.deepEqual(c.state.objects, expected);
});

test('sub-agent and model choice carry no authority of their own', () => {
  const b = bulk({ applyMode: 'allow' });
  startBulkJob(b.state, b.local, b.job);
  const items = b.items().map((i) => ({ ...i, subagent: 'haiku-worker', actingAs: 'owner' }));
  assert.equal(stageBulkBatch(b.state, b.local, 'job-1', 'b1', items).code, 'STAGED');
  assert.equal(stageBulkBatch(b.state, { ...b.local, clientId: 'client-A', credentialClass: 'cloud', transport: 'tunnel' }, 'job-1', 'b2', b.items()).code, 'DENIED');
  assert.equal(b.state.audit.at(-1).clientId, 'client-A');
});

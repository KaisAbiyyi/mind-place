// TEST-ONLY MP-7MCP-01 contract tests. No product endpoint is enabled by these fixtures.
import test from 'node:test';
import assert from 'node:assert/strict';
import { authorizeWrite, canRead, simulateWrite } from './support/mp-7mcp-authority-model.mjs';

const HASH = 'a'.repeat(64);
const HASH2 = 'b'.repeat(64);

function fixture(operation = 'create_import') {
  const state = {
    vaultId: 'vault-A', audience: 'mind-place-mcp', epoch: 17,
    policyRevision: 4, locked: false, paused: false, receipts: new Map(),
    clients: { 'client-A': { enabled: true, revoked: false }, 'client-B': { enabled: true, revoked: false } },
    grants: [], readGrants: [],
    objects: {
      prior: { vaultId: 'vault-A', collectionId: 'curated-A', objectKind: 'memory', revision: 3 }
    }
  };
  const auth = {
    valid: true, revoked: false, clientId: 'client-A', vaultId: 'vault-A',
    audience: 'mind-place-mcp', epoch: 17
  };
  const req = {
    clientId: 'client-A', operation, vaultId: 'vault-A', collectionId: 'curated-A',
    objectKind: 'memory', epoch: 17, policyRevision: 4, operationId: 'op-A',
    bodyDigest: HASH, now: 1000
  };
  const grant = (op = operation, collection = 'curated-A', client = 'client-A', kind = 'memory') => {
    state.grants.push({
      enabled: true, revoked: false, clientId: client, vaultId: 'vault-A',
      collectionId: collection, objectKind: kind, operation: op,
      policyRevision: 4, expiresAt: 3000
    });
  };
  return { state, auth, req, grant };
}

test('default deny, and granting write does not imply read', () => {
  const f = fixture();
  assert.equal(authorizeWrite(f.state, f.auth, f.req).code, 'DENIED');
  f.grant();
  assert.equal(authorizeWrite(f.state, f.auth, f.req).code, 'ALLOWED');
  assert.equal(canRead(f.state, f.auth, 'curated-A'), false);
  f.state.readGrants.push({
    enabled: true, revoked: false, clientId: 'client-A',
    vaultId: 'vault-A', collectionId: 'curated-A', policyRevision: 4
  });
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

test('wrong collection and wrong vault are denied without object enumeration', () => {
  const f = fixture();
  f.grant();
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, collectionId: 'curated-B' }).code, 'DENIED');
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, vaultId: 'vault-B' }).code, 'DENIED');
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, objectKind: 'source' }).code, 'DENIED');
});

test('only server-authenticated principal is trusted, not claimed client name', () => {
  const f = fixture();
  f.grant();
  assert.equal(authorizeWrite(f.state, { ...f.auth, clientId: 'client-B' }, f.req).code, 'DENIED');
  assert.equal(authorizeWrite(f.state, { ...f.auth, valid: false }, f.req).code, 'DENIED');
  assert.equal(authorizeWrite(f.state, { ...f.auth, audience: 'other' }, f.req).code, 'DENIED');
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, clientId: 'client-B' }).code, 'ALLOWED');
  // The simulated transport must supply f.auth from credential validation, not request JSON.
});

test('Lock, pause, epoch mismatch, revoked client and grant change fail closed', () => {
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
  f.state.clients['client-A'].revoked = false; f.state.policyRevision += 1;
  assert.equal(authorizeWrite(f.state, f.auth, f.req).code, 'DENIED');
});

test('expired grants, malformed digests and invalid operation are denied', () => {
  const f = fixture();
  f.grant();
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, now: 3000 }).code, 'DENIED');
  assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, bodyDigest: 'not-sha256' }).code, 'DENIED');
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
  const result = simulateWrite(f.state, f.auth, req);
  assert.equal(result.code, 'COMMITTED');
  assert.equal(f.state.objects.prior.collectionId, 'curated-B');
  assert.equal(f.state.objects.prior.revision, 4);
});

test('destructive operation denied without separate grant and confirmation', () => {
  const f = fixture('delete');
  const req = { ...f.req, objectId: 'prior', expectedRevision: 3 };
  assert.equal(authorizeWrite(f.state, f.auth, { ...req, ownerConfirmedDestruction: true }).code, 'DENIED');
  f.grant();
  assert.equal(authorizeWrite(f.state, f.auth, req).code, 'DENIED');
  const result = simulateWrite(f.state, f.auth, { ...req, ownerConfirmedDestruction: true });
  assert.equal(result.code, 'COMMITTED');
  assert.equal(f.state.objects.prior.tombstoned, true);
});

test('identical replay is one commit; changed body with same operation id conflicts', () => {
  const f = fixture();
  f.grant();
  const first = simulateWrite(f.state, f.auth, f.req);
  assert.equal(first.code, 'COMMITTED');
  assert.equal(f.state.objects['obj:client-A:op-A'].revision, 1);
  const again = simulateWrite(f.state, f.auth, f.req);
  assert.equal(again.code, 'REPLAY');
  assert.equal(f.state.objects['obj:client-A:op-A'].revision, 1);
  assert.equal(simulateWrite(f.state, f.auth, { ...f.req, bodyDigest: HASH2 }).code, 'CONFLICT');
  assert.equal(f.state.receipts.size, 1);
  assert.equal(JSON.stringify(first).includes('secret-content'), false);
});

test('replayed update bypasses old precondition only after fresh grant recheck', () => {
  const f = fixture('update');
  f.grant();
  const req = { ...f.req, objectId: 'prior', expectedRevision: 3 };
  assert.equal(simulateWrite(f.state, f.auth, req).code, 'COMMITTED');
  assert.equal(f.state.objects.prior.revision, 4);
  assert.equal(simulateWrite(f.state, f.auth, req).code, 'REPLAY');
  f.state.grants[0].revoked = true;
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
  const result = simulateWrite(f.state, f.auth, req, { beforeCommit(s) { s.epoch += 1; } });
  assert.equal(result.code, 'DENIED');
  assert.equal(f.state.objects.prior.revision, 3);
  assert.equal(f.state.receipts.size, 0);
});

test('revocation/grant policy revision fence between admission and commit prevents mutation', () => {
  const f = fixture('update');
  f.grant();
  const req = { ...f.req, objectId: 'prior', expectedRevision: 3 };
  const result = simulateWrite(f.state, f.auth, req, { beforeCommit(s) { s.grants[0].revoked = true; } });
  assert.equal(result.code, 'DENIED');
  assert.equal(f.state.objects.prior.revision, 3);
  assert.equal(f.state.receipts.size, 0);
});

test('committed but lost acknowledgement is reconciled by authorized retry', () => {
  const f = fixture();
  f.grant();
  assert.equal(simulateWrite(f.state, f.auth, f.req, { fault: 'after-commit' }).code, 'UNCERTAIN');
  assert.equal(f.state.receipts.size, 1);
  assert.equal(f.state.objects['obj:client-A:op-A'].revision, 1);
  assert.equal(simulateWrite(f.state, f.auth, f.req).code, 'REPLAY');
});

test('post-commit Lock prevents disclosing even the receipt; commit remains durable', () => {
  const f = fixture();
  f.grant();
  const result = simulateWrite(f.state, f.auth, f.req, { beforeDisclosure(s) { s.locked = true; } });
  assert.equal(result.code, 'DENIED');
  assert.equal(f.state.receipts.size, 1);
  assert.equal(simulateWrite(f.state, f.auth, f.req).code, 'DENIED');
});

test('no model grants external path, admin, or raw source mutation in policy operation set', () => {
  const f = fixture();
  f.grant();
  for (const operation of ['raw_delete', 'root_approve', 'sql_execute', 'filesystem_write', 'backup_restore', 'unlock']) {
    assert.equal(authorizeWrite(f.state, f.auth, { ...f.req, operation }).code, 'DENIED');
  }
});

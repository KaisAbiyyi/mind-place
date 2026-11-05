// MP-7MCP-01 reference policy model: TEST-ONLY, NOT a production security boundary.
// This file is intentionally isolated under test/support; no server/transport imports it.
// Real authorization must be enforced in authoritative Rust state/transport after owner approval.

const OPERATIONS = new Set(['create_import', 'append', 'update', 'move', 'delete']);
const DENIED = Object.freeze({ ok: false, code: 'DENIED' });
const CONFLICT = Object.freeze({ ok: false, code: 'CONFLICT' });
const ABORTED = Object.freeze({ ok: false, code: 'ABORTED' });

function sameGrant(grant, client, request, collectionId) {
  return grant.enabled === true &&
    grant.revoked !== true &&
    grant.clientId === client &&
    grant.vaultId === request.vaultId &&
    grant.collectionId === collectionId &&
    grant.objectKind === request.objectKind &&
    grant.operation === request.operation &&
    grant.policyRevision === request.policyRevision &&
    (grant.expiresAt === null || grant.expiresAt === undefined || grant.expiresAt > request.now);
}

export function authorizeWrite(state, authenticated, request, options = {}) {
  if (!state || !authenticated || !request || state.locked || state.paused) return DENIED;
  if (authenticated.valid !== true || authenticated.revoked === true ||
      authenticated.audience !== state.audience || authenticated.vaultId !== state.vaultId ||
      authenticated.epoch !== state.epoch || request.epoch !== state.epoch ||
      request.vaultId !== state.vaultId || request.policyRevision !== state.policyRevision ||
      !OPERATIONS.has(request.operation) || !Number.isFinite(request.now) ||
      typeof request.operationId !== 'string' || !request.operationId ||
      typeof request.bodyDigest !== 'string' || !/^[a-f0-9]{64}$/.test(request.bodyDigest) ||
      typeof request.collectionId !== 'string' || !request.collectionId ||
      typeof request.objectKind !== 'string' || !request.objectKind ||
      typeof authenticated.clientId !== 'string' || !authenticated.clientId) return DENIED;

  const client = state.clients?.[authenticated.clientId];
  if (!client || client.enabled !== true || client.revoked === true) return DENIED;
  // Request-supplied claimed identity is deliberately ignored; only server-authenticated principal is used.
  const hasGrant = (collection) => state.grants?.some(
    (grant) => sameGrant(grant, authenticated.clientId, request, collection)
  ) === true;
  if (!hasGrant(request.collectionId)) return DENIED;
  if (request.operation === 'delete' && request.ownerConfirmedDestruction !== true) return DENIED;

  const requiresExisting = ['update', 'move', 'delete'].includes(request.operation);
  const existing = request.objectId === undefined ? undefined : state.objects?.[request.objectId];
  if (!options.skipObjectPrecondition && (requiresExisting || request.operation === 'append')) {
    if (!existing || existing.vaultId !== request.vaultId ||
        existing.objectKind !== request.objectKind ||
        existing.collectionId !== request.collectionId) return DENIED;
    if (request.expectedRevision !== existing.revision) return CONFLICT;
  }
  if (request.operation === 'move') {
    if (typeof request.destinationCollectionId !== 'string' ||
        !request.destinationCollectionId || !hasGrant(request.destinationCollectionId)) return DENIED;
  }
  if (!options.skipObjectPrecondition && request.operation === 'create_import' && request.objectId !== undefined &&
      state.objects?.[request.objectId]) return CONFLICT;
  return { ok: true, code: 'ALLOWED' };
}

export function canRead(state, authenticated, collectionId) {
  if (!state || !authenticated || state.locked || state.paused ||
      !authenticated.valid || authenticated.revoked ||
      authenticated.vaultId !== state.vaultId || authenticated.epoch !== state.epoch ||
      authenticated.audience !== state.audience ||
      !state.clients?.[authenticated.clientId]?.enabled ||
      state.clients?.[authenticated.clientId]?.revoked) return false;
  return state.readGrants?.some((grant) =>
    grant.clientId === authenticated.clientId && grant.vaultId === state.vaultId &&
    grant.collectionId === collectionId && grant.enabled === true &&
    grant.revoked !== true && grant.policyRevision === state.policyRevision
  ) === true;
}

export function simulateWrite(state, authenticated, request, options = {}) {
  const initial = authorizeWrite(state, authenticated, request);
  if (!initial.ok) return initial;

  const receiptKey = JSON.stringify([state.vaultId, authenticated.clientId, request.operationId]);
  const previous = state.receipts.get(receiptKey);
  if (previous) {
    if (previous.bodyDigest !== request.bodyDigest ||
        previous.operation !== request.operation ||
        previous.collectionId !== request.collectionId ||
        previous.objectKind !== request.objectKind ||
        previous.destinationCollectionId !== (request.destinationCollectionId ?? null)) return CONFLICT;
    // Existing receipt is disclosed only after *fresh* authorization, never merely by operation_id.
    return { ok: true, code: 'REPLAY', receipt: { ...previous } };
  }

  // Stage is not visible in authoritative objects/receipts.
  if (options.fault === 'before-commit') return ABORTED;
  if (typeof options.beforeCommit === 'function') options.beforeCommit(state);
  const atCommit = authorizeWrite(state, authenticated, request);
  if (!atCommit.ok) return atCommit;

  const id = request.objectId ?? 'obj:' + authenticated.clientId + ':' + request.operationId;
  const old = state.objects[id];
  const nextRevision = (old?.revision ?? 0) + 1;
  state.objects[id] = {
    vaultId: state.vaultId,
    collectionId: request.operation === 'move' ? request.destinationCollectionId : request.collectionId,
    objectKind: request.objectKind,
    revision: nextRevision,
    tombstoned: request.operation === 'delete'
  };
  const receipt = {
    operation: request.operation,
    collectionId: request.collectionId,
    objectKind: request.objectKind,
    destinationCollectionId: request.destinationCollectionId ?? null,
    bodyDigest: request.bodyDigest,
    revision: nextRevision,
    receiptId: receiptKey
  };
  state.receipts.set(receiptKey, receipt);

  if (options.fault === 'after-commit') return { ok: false, code: 'UNCERTAIN' };
  if (typeof options.beforeDisclosure === 'function') options.beforeDisclosure(state);
  if (!authorizeWrite(state, authenticated, request, { skipObjectPrecondition: true }).ok) return DENIED;
  return { ok: true, code: 'COMMITTED', receipt: { ...receipt } };
}

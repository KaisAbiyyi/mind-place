// MP-7MCP-01 reference policy model v2: TEST-ONLY, NOT a production security boundary.
// Isolated under test/support; no server/transport imports it. Real authorization must be
// enforced in authoritative Rust state/transport after owner approval of the final contract.
import { createHash } from 'node:crypto';

export const OPERATIONS = Object.freeze(['create_import', 'append', 'update', 'move', 'delete']); // delete = tombstone only
export const BULK_OPERATIONS = Object.freeze(['bulk_stage', 'bulk_apply']);
const BULK_ITEM_OPERATIONS = new Set(['create_import', 'append', 'update']);
const MODES = new Set(['allow', 'ask_first', 'deny']);
const TIERS = new Set(['cloud', 'local']);
const ID = /^[A-Za-z0-9_.:-]+$/;
const HOUR_MS = 3_600_000;

// Object kinds belong to exactly one layer. Unknown kinds are denied.
export const KIND_LAYER = Object.freeze({
  memory: 'curated', route: 'curated', note: 'curated',
  graph_batch: 'derived',
  raw_capture: 'raw', raw_import: 'raw', source: 'raw'
});
// Operation x layer eligibility (defence in depth below grants). Raw evidence is immutable.
const ELIGIBLE = Object.freeze({
  raw: new Set(['create_import', 'append']),
  curated: new Set(OPERATIONS),
  derived: new Set(OPERATIONS)
});
const CLOUD_LAYERS = new Set(['curated']);

// Owner-settable defaults. Cloud numbers confirmed by the owner; bulk numbers mirror
// crates/index-core/src/graph_maintenance.rs (MAX_LIFETIME/MAX_BATCH/MAX_ITEMS).
export const CLOUD_DEFAULTS = Object.freeze({
  maxGrantMs: 30 * 24 * HOUR_MS, maxBytes: 1_048_576, maxObjects: 20, maxWritesPerHour: 60
});
export const BULK_DEFAULTS = Object.freeze({ leaseMs: 15 * 60 * 1000, maxBatch: 512, maxItems: 20_000 });

const R = (code, ok = false) => Object.freeze({ ok, code });
const DENIED = R('DENIED');
const CONFLICT = R('CONFLICT');
const ABORTED = R('ABORTED');
const LIMIT = R('LIMIT');
const APPROVAL_REQUIRED = R('APPROVAL_REQUIRED');

function canonical(value) {
  if (Array.isArray(value)) return '[' + value.map(canonical).join(',') + ']';
  if (value && typeof value === 'object') {
    return '{' + Object.keys(value).sort().map((k) => JSON.stringify(k) + ':' + canonical(value[k])).join(',') + '}';
  }
  return JSON.stringify(value === undefined ? null : value);
}
const sha256 = (text) => createHash('sha256').update(text).digest('hex');

// The digest is computed by the SERVER over the canonical request; a client-supplied digest is never trusted.
export function digestOf(request) {
  return sha256(canonical({
    operation: request.operation, vaultId: request.vaultId, collectionId: request.collectionId,
    objectKind: request.objectKind, objectId: request.objectId ?? null,
    destinationCollectionId: request.destinationCollectionId ?? null,
    expectedRevision: request.expectedRevision ?? null, payload: request.payload
  }));
}

// Identity comes only from server-side credential validation (auth), never from request fields.
function principal(state, auth) {
  if (!state || !auth || state.locked || state.paused || state.authorityReadable === false) return { ok: false };
  if (!Number.isFinite(state.now)) return { ok: false };
  if (auth.valid !== true || auth.revoked === true || auth.audience !== state.audience ||
      auth.vaultId !== state.vaultId || auth.epoch !== state.epoch ||
      typeof auth.clientId !== 'string' || !auth.clientId) return { ok: false };
  const client = state.clients?.[auth.clientId];
  if (!client || client.enabled !== true || client.revoked === true) return { ok: false };
  if (!TIERS.has(client.tier) || auth.credentialClass !== client.tier) return { ok: false };
  // Local-tier credentials are valid only on the local transport, never through the hosted tunnel.
  if (client.tier === 'local' && auth.transport !== 'loopback') return { ok: false };
  return { ok: true, client };
}

function findGrant(state, client, auth, spec) {
  return state.grants?.find((g) =>
    g.enabled === true && g.revoked !== true && g.clientId === auth.clientId &&
    g.vaultId === state.vaultId && g.collectionId === spec.collectionId &&
    g.objectKind === spec.objectKind && g.operation === spec.operation &&
    g.policyRevision === client.writePolicyRevision &&
    (g.mode === 'allow' || g.mode === 'ask_first') &&
    (client.tier === 'cloud' ? Number.isFinite(g.expiresAt) && g.expiresAt > state.now
      : g.expiresAt === undefined || g.expiresAt === null || g.expiresAt > state.now)
  ) ?? null;
}

function findApproval(state, client, auth, spec) {
  return state.approvals?.find((a) =>
    a.used !== true && a.ownerApproved === true && a.clientId === auth.clientId &&
    a.vaultId === state.vaultId && a.collectionId === spec.collectionId &&
    a.operation === spec.operation && a.operationId === spec.operationId &&
    a.bodyDigest === spec.digest && a.policyRevision === client.writePolicyRevision &&
    a.epoch === state.epoch && a.expiresAt > state.now
  ) ?? null;
}

const recentWrites = (state, grantId) =>
  (state.writeLog ?? []).filter((w) => w.grantId === grantId && w.at > state.now - HOUR_MS).length;

function audit(state, auth, request, result) {
  (state.audit ??= []).push({
    at: state.now, clientId: auth?.clientId ?? null, operation: request?.operation ?? null,
    collectionId: request?.collectionId ?? null, decision: result.ok ? 'allow' : 'deny', code: result.code
  });
  return result;
}

// Grant issuance validator (owner UI / policy store side).
export function validateGrant(grant, client, now) {
  const bad = (reason) => ({ ok: false, reason });
  if (!grant || !client) return bad('missing');
  for (const f of ['id', 'clientId', 'vaultId', 'collectionId', 'objectKind', 'operation']) {
    if (typeof grant[f] !== 'string' || !ID.test(grant[f])) return bad('field:' + f);
  }
  if (!OPERATIONS.includes(grant.operation) && !BULK_OPERATIONS.includes(grant.operation)) return bad('operation');
  if (!MODES.has(grant.mode)) return bad('mode');
  const layer = KIND_LAYER[grant.objectKind];
  if (!layer) return bad('kind');
  if (OPERATIONS.includes(grant.operation) && !ELIGIBLE[layer].has(grant.operation)) return bad('ineligible');
  if (BULK_OPERATIONS.includes(grant.operation) && layer === 'raw') return bad('ineligible');
  if (client.tier === 'cloud') {
    if (BULK_OPERATIONS.includes(grant.operation)) return bad('cloud-bulk');
    if (!CLOUD_LAYERS.has(layer)) return bad('cloud-layer');
    const limit = grant.limits?.maxGrantMs ?? CLOUD_DEFAULTS.maxGrantMs;
    if (!Number.isFinite(grant.expiresAt) || grant.expiresAt - now > limit) return bad('cloud-expiry');
  }
  return { ok: true };
}

export function authorizeWrite(state, auth, request, options = {}) {
  const p = principal(state, auth);
  if (!p.ok || !request || typeof request !== 'object') return DENIED;
  const client = p.client;
  if (!OPERATIONS.includes(request.operation) || request.bulk === true) return DENIED;
  for (const f of ['vaultId', 'collectionId', 'objectKind', 'operationId']) {
    if (typeof request[f] !== 'string' || !request[f]) return DENIED;
  }
  if (typeof request.payload !== 'string' || request.vaultId !== state.vaultId) return DENIED;
  const layer = KIND_LAYER[request.objectKind];
  if (!layer || !ELIGIBLE[layer].has(request.operation)) return DENIED;
  if (client.tier === 'cloud' && !CLOUD_LAYERS.has(layer)) return DENIED;
  if (state.collections?.[request.collectionId]?.layer !== layer) return DENIED;
  const grant = findGrant(state, client, auth, request);
  if (!grant) return DENIED;
  if (request.operation === 'create_import' && request.objectId !== undefined) return DENIED; // server assigns ids

  const digest = digestOf(request);
  if (!options.replay) {
    const limits = { ...(client.tier === 'cloud' ? CLOUD_DEFAULTS : {}), ...(grant.limits ?? {}) };
    if (limits.maxBytes !== undefined && Buffer.byteLength(request.payload) > limits.maxBytes) return LIMIT;
    if (limits.maxObjects !== undefined && (request.objectCount ?? 1) > limits.maxObjects) return LIMIT;
    if (limits.maxWritesPerHour !== undefined && recentWrites(state, grant.id) >= limits.maxWritesPerHour) return LIMIT;
    if (request.operation !== 'create_import') {
      const existing = request.objectId === undefined ? undefined : state.objects?.[request.objectId];
      if (!existing || existing.vaultId !== request.vaultId || existing.objectKind !== request.objectKind ||
          existing.collectionId !== request.collectionId || existing.tombstoned === true) return DENIED;
      if (request.expectedRevision !== existing.revision) return CONFLICT;
    }
    // delete (tombstone) always needs a single-use owner approval; ask_first needs one for any operation.
    if ((request.operation === 'delete' || grant.mode === 'ask_first') &&
        !findApproval(state, client, auth, { ...request, digest })) return APPROVAL_REQUIRED;
  }
  if (request.operation === 'move') {
    const dest = request.destinationCollectionId;
    if (typeof dest !== 'string' || !dest || state.collections?.[dest]?.layer !== layer ||
        !findGrant(state, client, auth, { collectionId: dest, objectKind: request.objectKind, operation: 'move' })) return DENIED;
  }
  return { ok: true, code: 'ALLOWED', digest, grantId: grant.id };
}

export function canRead(state, auth, collectionId) {
  const p = principal(state, auth);
  if (!p.ok) return false;
  const layer = state.collections?.[collectionId]?.layer;
  if (!layer || (p.client.tier === 'cloud' && layer === 'raw')) return false;
  return state.readGrants?.some((g) =>
    g.clientId === auth.clientId && g.vaultId === state.vaultId && g.collectionId === collectionId &&
    g.enabled === true && g.revoked !== true && g.policyRevision === p.client.readPolicyRevision) === true;
}

function currentReceiptScope(state, auth, request) {
  const id = request.objectId ?? 'obj:' + auth.clientId + ':' + request.operationId;
  const object = state.objects?.[id];
  const allowed = request.operation === 'move' ? request.destinationCollectionId : request.collectionId;
  return !!object && object.vaultId === state.vaultId && object.collectionId === allowed &&
    object.objectKind === request.objectKind;
}

const snapshot = (o) => ({ revision: o.revision, collectionId: o.collectionId, data: o.data, tombstoned: o.tombstoned === true });

export function simulateWrite(state, auth, request, options = {}) {
  return audit(state, auth, request, runWrite(state, auth, request, options));
}

function runWrite(state, auth, request, options) {
  if (!auth?.clientId || typeof request?.operationId !== 'string' || typeof request?.payload !== 'string') return DENIED;
  const receiptKey = JSON.stringify([state.vaultId, auth.clientId, request.operationId]);
  const previous = state.receipts.get(receiptKey);
  const initial = authorizeWrite(state, auth, request, { replay: !!previous });
  if (!initial.ok) return initial;
  if (previous) {
    if (previous.bodyDigest !== initial.digest || previous.operation !== request.operation) return CONFLICT;
    if (!currentReceiptScope(state, auth, request)) return DENIED;
    return { ok: true, code: 'REPLAY', receipt: { ...previous } };
  }
  if (options.fault === 'before-commit') return ABORTED;
  if (typeof options.beforeCommit === 'function') options.beforeCommit(state);
  const atCommit = authorizeWrite(state, auth, request);
  if (!atCommit.ok) return atCommit;

  const approval = (request.operation === 'delete' || findGrant(state, state.clients[auth.clientId], auth, request)?.mode === 'ask_first')
    ? findApproval(state, state.clients[auth.clientId], auth, { ...request, digest: atCommit.digest }) : null;
  if (approval) approval.used = true; // single-use
  (state.writeLog ??= []).push({ grantId: atCommit.grantId, at: state.now });

  const id = request.objectId ?? 'obj:' + auth.clientId + ':' + request.operationId;
  const old = state.objects[id];
  const next = {
    vaultId: state.vaultId,
    collectionId: request.operation === 'move' ? request.destinationCollectionId : request.collectionId,
    objectKind: request.objectKind,
    revision: (old?.revision ?? 0) + 1,
    tombstoned: request.operation === 'delete',
    data: request.operation === 'append' ? (old?.data ?? '') + '\n' + request.payload
      : request.operation === 'update' || request.operation === 'create_import' ? request.payload : old?.data,
    history: old ? [...(old.history ?? []), snapshot(old)] : []
  };
  state.objects[id] = next;
  const receipt = {
    operation: request.operation, collectionId: request.collectionId, objectKind: request.objectKind,
    destinationCollectionId: request.destinationCollectionId ?? null, bodyDigest: atCommit.digest,
    revision: next.revision, receiptId: receiptKey
  };
  state.receipts.set(receiptKey, receipt);
  if (options.fault === 'after-commit') return R('UNCERTAIN');
  if (typeof options.beforeDisclosure === 'function') options.beforeDisclosure(state);
  if (!authorizeWrite(state, auth, request, { replay: true }).ok || !currentReceiptScope(state, auth, request)) return DENIED;
  return { ok: true, code: 'COMMITTED', receipt: { ...receipt } };
}

// Restore and rollback are OWNER-ONLY local-UI actions. They are not MCP operations.
const ownerOk = (state, owner) => !!state && !state.locked && owner?.localUi === true && owner.authenticated === true;

export function ownerRestoreTombstone(state, owner, objectId) {
  const o = state?.objects?.[objectId];
  if (!ownerOk(state, owner) || !o || o.tombstoned !== true) return audit(state ?? {}, { clientId: 'owner' }, { operation: 'restore' }, DENIED);
  state.objects[objectId] = { ...o, tombstoned: false, revision: o.revision + 1, history: [...(o.history ?? []), snapshot(o)] };
  return audit(state, { clientId: 'owner' }, { operation: 'restore' }, R('RESTORED', true));
}

// ---- Local-tier bulk jobs: out-of-band, scoped, staged, applied separately, rolled back by supersession ----
const jobKey = (auth, jobId) => auth.clientId + ':' + jobId;
const audienceCovers = (source, target) => Array.isArray(source) && Array.isArray(target) && target.every((r) => source.includes(r));

// Scope is the owner/prompt-declared project boundary; every source needs a CURRENT read grant. Ordinary raw -> curated promotion is
// the purpose of a bulk job (gated by tier + apply approval); a PRIVATE source may only feed a target whose readers it also covers.
// Evaluated at job start AND again inside the apply commit, so revoking a source after staging blocks the batch.
function scopeAuthorized(state, auth, targetId, scope) {
  const target = state.collections?.[targetId];
  if (!target) return false;
  return scope.every((c) => {
    const source = state.collections?.[c];
    return !!source && canRead(state, auth, c) && !(source.private === true && !audienceCovers(source.audience, target.audience));
  });
}

export function startBulkJob(state, auth, job) {
  const p = principal(state, auth);
  const res = (r) => audit(state ?? {}, auth, { operation: 'bulk_start', collectionId: job?.targetCollectionId }, r);
  if (!p.ok || p.client.tier !== 'local' || typeof job?.jobId !== 'string' || !job.jobId ||
      !Array.isArray(job.scopeCollections) || job.scopeCollections.length === 0) return res(DENIED);
  const layer = KIND_LAYER[job.objectKind];
  if (!layer || layer === 'raw' || state.collections?.[job.targetCollectionId]?.layer !== layer) return res(DENIED);
  const grant = findGrant(state, p.client, auth, { collectionId: job.targetCollectionId, objectKind: job.objectKind, operation: 'bulk_stage' });
  if (!grant) return res(DENIED);
  if (!scopeAuthorized(state, auth, job.targetCollectionId, job.scopeCollections)) return res(DENIED);
  const key = jobKey(auth, job.jobId);
  if (state.jobs?.[key]) return res(CONFLICT);
  const limits = { ...BULK_DEFAULTS, ...(grant.limits ?? {}) };
  (state.jobs ??= {})[key] = {
    clientId: auth.clientId, epoch: state.epoch, policyRevision: p.client.writePolicyRevision,
    target: job.targetCollectionId, kind: job.objectKind, scope: [...job.scopeCollections],
    expiresAt: state.now + limits.leaseMs, limits, itemCount: 0, status: 'open'
  };
  return res({ ok: true, code: 'STARTED', expiresAt: state.jobs[key].expiresAt });
}

function openJob(state, auth, jobId, { leaseRequired }) {
  const p = principal(state, auth);
  if (!p.ok || p.client.tier !== 'local') return null;
  const job = state.jobs?.[jobKey(auth, jobId)];
  if (!job || job.status !== 'open' || job.epoch !== state.epoch || job.policyRevision !== p.client.writePolicyRevision) return null;
  if (leaseRequired && state.now >= job.expiresAt) return null;
  return { p, job };
}

function itemEligible(state, p, auth, job, item) {
  if (!BULK_ITEM_OPERATIONS.has(item.operation) || typeof item.payload !== 'string') return DENIED;
  if (!findGrant(state, p.client, auth, { collectionId: job.target, objectKind: job.kind, operation: item.operation })) return DENIED;
  if (item.operation === 'create_import') return item.objectId === undefined ? null : DENIED;
  const o = state.objects?.[item.objectId];
  if (!o || o.vaultId !== state.vaultId || o.collectionId !== job.target || o.objectKind !== job.kind || o.tombstoned === true) return DENIED;
  return item.expectedRevision === o.revision ? null : CONFLICT;
}

export function stageBulkBatch(state, auth, jobId, batchId, items) {
  const res = (r) => audit(state ?? {}, auth, { operation: 'bulk_stage', collectionId: null }, r);
  const ctx = typeof batchId === 'string' && batchId && Array.isArray(items) && items.length ? openJob(state, auth, jobId, { leaseRequired: true }) : null;
  if (!ctx) return res(DENIED);
  const { p, job } = ctx;
  if (items.length > job.limits.maxBatch || job.itemCount + items.length > job.limits.maxItems) return res(LIMIT);
  const key = auth.clientId + ':' + batchId;
  if (state.batches?.[key]) return res(CONFLICT);
  const seen = new Set();
  for (const item of items) {
    if (!Array.isArray(item.evidence) || item.evidence.length === 0 || !Array.isArray(item.sources) || item.sources.length === 0) return res(DENIED);
    if (![...item.sources, ...item.evidence.map((e) => e.collectionId)].every((c) => job.scope.includes(c))) return res(DENIED);
    if (item.objectId !== undefined) { if (seen.has(item.objectId)) return res(DENIED); seen.add(item.objectId); }
    const bad = itemEligible(state, p, auth, job, item);
    if (bad) return res(bad);
  }
  const normalized = items.map((i) => ({ operation: i.operation, objectId: i.objectId ?? null, expectedRevision: i.expectedRevision ?? null, payload: i.payload, evidence: i.evidence, sources: i.sources }));
  (state.batches ??= {})[key] = { jobKey: jobKey(auth, jobId), items: normalized, digest: sha256(canonical(normalized)), status: 'staged', evidenceCount: items.reduce((n, i) => n + i.evidence.length, 0) };
  job.itemCount += items.length;
  return res({ ok: true, code: 'STAGED', batchDigest: state.batches[key].digest }); // staged items are NOT visible in state.objects
}

export function applyBulkBatch(state, auth, jobId, batchId, options = {}) {
  const res = (r) => audit(state ?? {}, auth, { operation: 'bulk_apply', collectionId: null }, r);
  const ctx = openJob(state, auth, jobId, { leaseRequired: false });
  const batch = ctx && state.batches?.[auth.clientId + ':' + batchId];
  if (!batch || batch.status !== 'staged') return res(DENIED);
  const { p, job } = ctx;
  const spec = { collectionId: job.target, operation: 'bulk_apply', operationId: batchId, digest: batch.digest };
  // Strictest policy wins: the batch needs an owner approval if bulk_apply OR any operation inside it is ask_first. One approval,
  // bound to the batch digest, satisfies all of them; a bulk grant can never weaken a per-operation policy. Re-evaluated at commit.
  const approvalNeeded = () => findGrant(state, p.client, auth, { collectionId: job.target, objectKind: job.kind, operation: 'bulk_apply' })?.mode === 'ask_first' ||
    batch.items.some((i) => findGrant(state, p.client, auth, { collectionId: job.target, objectKind: job.kind, operation: i.operation })?.mode === 'ask_first');
  const verify = () => {
    if (!principal(state, auth).ok || job.epoch !== state.epoch || job.policyRevision !== p.client.writePolicyRevision) return DENIED;
    if (!findGrant(state, p.client, auth, { collectionId: job.target, objectKind: job.kind, operation: 'bulk_apply' })) return DENIED;
    if (!scopeAuthorized(state, auth, job.target, job.scope)) return DENIED;
    for (const item of batch.items) {
      const bad = itemEligible(state, p, auth, job, { ...item, objectId: item.objectId ?? undefined, expectedRevision: item.expectedRevision ?? undefined });
      if (bad) return bad;
    }
    if (approvalNeeded() && !findApproval(state, p.client, auth, spec)) return APPROVAL_REQUIRED;
    return null;
  };
  let bad = verify();
  if (bad) return res(bad);
  if (typeof options.beforeCommit === 'function') options.beforeCommit(state);
  bad = verify(); // all-or-nothing: re-authorize every item inside the commit
  if (bad) return res(bad);
  if (approvalNeeded()) findApproval(state, p.client, auth, spec).used = true; // single-use, bound to this batch digest
  const created = []; const changed = [];
  batch.items.forEach((item, index) => {
    if (item.operation === 'create_import') {
      const id = 'obj:' + auth.clientId + ':' + batchId + ':' + index;
      state.objects[id] = { vaultId: state.vaultId, collectionId: job.target, objectKind: job.kind, revision: 1, tombstoned: false, data: item.payload, history: [], batchId };
      created.push({ id, resultRevision: 1 });
    } else {
      const old = state.objects[item.objectId];
      const next = { ...old, revision: old.revision + 1, history: [...(old.history ?? []), snapshot(old)], batchId,
        data: item.operation === 'append' ? (old.data ?? '') + '\n' + item.payload : item.payload };
      state.objects[item.objectId] = next;
      changed.push({ id: item.objectId, before: snapshot(old), resultRevision: next.revision });
    }
  });
  batch.status = 'applied'; batch.applied = { created, changed };
  return res({ ok: true, code: 'APPLIED', created: created.length, changed: changed.length });
}

// Rollback is owner-only and logical: created objects are tombstoned, changed objects get a NEW revision restoring prior content.
export function ownerRollbackBatch(state, owner, clientId, batchId) {
  const batch = state?.batches?.[clientId + ':' + batchId];
  const res = (r) => audit(state ?? {}, { clientId: 'owner' }, { operation: 'bulk_rollback' }, r);
  if (!ownerOk(state, owner) || !batch || batch.status !== 'applied') return res(DENIED);
  const { created, changed } = batch.applied;
  for (const e of [...created, ...changed]) if (state.objects[e.id]?.revision !== e.resultRevision) return res(CONFLICT);
  for (const e of created) {
    const o = state.objects[e.id];
    state.objects[e.id] = { ...o, tombstoned: true, revision: o.revision + 1, history: [...o.history, snapshot(o)] };
  }
  for (const e of changed) {
    const o = state.objects[e.id];
    state.objects[e.id] = { ...o, data: e.before.data, collectionId: e.before.collectionId, revision: o.revision + 1, history: [...o.history, snapshot(o)] };
  }
  batch.status = 'rolled_back';
  return res(R('ROLLED_BACK', true));
}

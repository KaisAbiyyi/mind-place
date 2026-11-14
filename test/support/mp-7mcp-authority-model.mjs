// MP-7MCP-01 reference policy model v3: TEST-ONLY, NOT a production security boundary.
// Isolated under test/support; no server/transport imports it. Real authorization must be
// enforced in authoritative Rust state/transport after owner approval of the final contract.
import { createHash } from 'node:crypto';

export const OPERATIONS = Object.freeze(['create_import', 'append', 'update', 'move', 'delete']); // delete = tombstone only
export const BULK_OPERATIONS = Object.freeze(['bulk_stage', 'bulk_apply']);
const BULK_ITEM_OPERATIONS = new Set(['create_import', 'append', 'update']);
const MODES = new Set(['allow', 'ask_first', 'deny']);
const TIERS = new Set(['cloud', 'local']);
const ID = /^[A-Za-z0-9_.:-]+$/;
const MIN_MS = 60_000;
const HOUR_MS = 60 * MIN_MS;
const DAY_MS = 24 * HOUR_MS;

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

// ---- three-level limit model (C1) ------------------------------------------------------------
// 1. DEFAULT_LIMITS: safe defaults used when nothing is configured.
// 2. Owner hard caps (state.ownerPolicy.caps, set only through setOwnerCaps): >= default, <= ceiling.
//    FACTORY_OWNER_CAPS applies until the owner configures a cap. Grants/jobs may override a limit
//    downward or up to the CURRENT cap, never above it.
// 3. ENGINE_CEILINGS: compile-time safety ceilings protecting against resource exhaustion. No policy,
//    grant or request can exceed them. maxBytes mirrors the CURRENT production MCP request envelope
//    (BODY_LIMIT / MAX_REQUEST_BYTES = 64 KiB) minus JSON-RPC framing headroom; raising it is a Stage B
//    transport decision and is deliberately NOT done here.
export const LIMIT_KEYS = Object.freeze([
  'maxBytes', 'maxObjects', 'maxWritesPerHour', 'maxGrantMs',
  'leaseMs', 'maxBatch', 'maxItems', 'maxConcurrentWorkers', 'proposalTtlMs', 'approvalTtlMs'
]);
export const DEFAULT_LIMITS = Object.freeze({
  maxBytes: 32_768, maxObjects: 20, maxWritesPerHour: 60, maxGrantMs: 30 * DAY_MS,
  leaseMs: 15 * MIN_MS, maxBatch: 512, maxItems: 20_000, maxConcurrentWorkers: 3,
  proposalTtlMs: 7 * DAY_MS, approvalTtlMs: 10 * MIN_MS
});
export const FACTORY_OWNER_CAPS = Object.freeze({
  maxBytes: 49_152, maxObjects: 50, maxWritesPerHour: 600, maxGrantMs: 60 * DAY_MS,
  leaseMs: 30 * MIN_MS, maxBatch: 1_024, maxItems: 50_000, maxConcurrentWorkers: 5,
  proposalTtlMs: 14 * DAY_MS, approvalTtlMs: HOUR_MS
});
export const ENGINE_CEILINGS = Object.freeze({
  maxBytes: 49_152, maxObjects: 100, maxWritesPerHour: 3_600, maxGrantMs: 90 * DAY_MS,
  leaseMs: HOUR_MS, maxBatch: 2_048, maxItems: 100_000, maxConcurrentWorkers: 8,
  proposalTtlMs: 30 * DAY_MS, approvalTtlMs: DAY_MS
});
// Version of the deterministic risk-classification rules; approvals are bound to it.
export const RISK_POLICY_VERSION = 1;

const validLimit = (v) => Number.isSafeInteger(v) && v >= 1;

// Trusted owner caps. Anything malformed in storage falls back to the safe default, never to a higher value.
function ownerCaps(state) {
  const stored = state?.ownerPolicy?.caps ?? {};
  const caps = {};
  for (const k of LIMIT_KEYS) {
    const v = stored[k];
    const base = validLimit(v) && v >= DEFAULT_LIMITS[k] ? v : (v === undefined ? FACTORY_OWNER_CAPS[k] : DEFAULT_LIMITS[k]);
    caps[k] = Math.min(base, ENGINE_CEILINGS[k]);
  }
  return caps;
}

// Effective limits are recomputed from CURRENT policy on every use, so cap changes apply immediately.
// An override above the current cap is clamped here (enforcement); the issuance validator rejects it.
export function effectiveLimits(state, grant) {
  const overrides = grant?.limits;
  if (overrides !== undefined && (overrides === null || typeof overrides !== 'object' || Array.isArray(overrides))) return { ok: false };
  for (const k of Object.keys(overrides ?? {})) if (!LIMIT_KEYS.includes(k)) return { ok: false };
  const caps = ownerCaps(state);
  const limits = {};
  for (const k of LIMIT_KEYS) {
    const v = overrides?.[k];
    if (v !== undefined && !validLimit(v)) return { ok: false };
    limits[k] = Math.min(v ?? DEFAULT_LIMITS[k], caps[k]);
  }
  return { ok: true, limits, caps };
}

function canonical(value) {
  if (Array.isArray(value)) return '[' + value.map(canonical).join(',') + ']';
  if (value && typeof value === 'object') {
    return '{' + Object.keys(value).sort().map((k) => JSON.stringify(k) + ':' + canonical(value[k])).join(',') + '}';
  }
  return JSON.stringify(value === undefined ? null : value);
}
const sha256 = (text) => createHash('sha256').update(text).digest('hex');

const R = (code, ok = false) => Object.freeze({ ok, code });
const DENIED = R('DENIED');
const CONFLICT = R('CONFLICT');
const ABORTED = R('ABORTED');
const LIMIT = R('LIMIT');
const INVALID = R('INVALID');
const EXPIRED = R('EXPIRED');
const STALE_EVIDENCE = R('STALE_EVIDENCE');
const STALE_SUMMARY = R('STALE_SUMMARY');
const DUPLICATE = R('DUPLICATE');
const APPROVAL_REQUIRED = R('APPROVAL_REQUIRED');

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

function grantLifetimeOk(state, client, g) {
  const eff = effectiveLimits(state, g);
  if (!eff.ok) return false;
  if (client.tier === 'cloud') {
    // Cloud grants must carry an expiry, and the lifetime is re-checked against the CURRENT owner cap.
    return Number.isFinite(g.expiresAt) && g.expiresAt > state.now && Number.isFinite(g.issuedAt) &&
      g.expiresAt - g.issuedAt <= eff.limits.maxGrantMs;
  }
  return g.expiresAt === undefined || g.expiresAt === null || g.expiresAt > state.now;
}

function findGrant(state, client, auth, spec) {
  return state.grants?.find((g) =>
    g.enabled === true && g.revoked !== true && g.clientId === auth.clientId &&
    g.vaultId === state.vaultId && g.collectionId === spec.collectionId &&
    g.objectKind === spec.objectKind && g.operation === spec.operation &&
    g.policyRevision === client.writePolicyRevision &&
    (g.mode === 'allow' || g.mode === 'ask_first') && grantLifetimeOk(state, client, g)
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

// Grant issuance validator (owner UI / policy store side). Overrides must respect the CURRENT owner caps.
export function validateGrant(grant, client, now, state) {
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
  if (grant.limits !== undefined) {
    if (grant.limits === null || typeof grant.limits !== 'object' || Array.isArray(grant.limits)) return bad('limits');
    const caps = ownerCaps(state);
    for (const [k, v] of Object.entries(grant.limits)) {
      if (!LIMIT_KEYS.includes(k)) return bad('limit-key:' + k);
      if (!validLimit(v)) return bad('limit-invalid:' + k);
      if (v > Math.min(caps[k], ENGINE_CEILINGS[k])) return bad('limit-over-cap:' + k);
    }
  }
  if (client.tier === 'cloud') {
    if (BULK_OPERATIONS.includes(grant.operation)) return bad('cloud-bulk');
    if (!CLOUD_LAYERS.has(layer)) return bad('cloud-layer');
    const allowed = grant.limits?.maxGrantMs ?? DEFAULT_LIMITS.maxGrantMs;
    if (!Number.isFinite(grant.expiresAt) || !Number.isFinite(grant.issuedAt) || grant.issuedAt > now ||
        grant.expiresAt - grant.issuedAt > allowed) return bad('cloud-expiry');
  }
  return { ok: true };
}

// Owner-only (trusted local UI) hard-cap management. Never reachable over MCP.
const ownerOk = (state, owner) => !!state && !state.locked && owner?.localUi === true && owner.authenticated === true;

export function setOwnerCaps(state, owner, caps) {
  const res = (r) => audit(state ?? {}, { clientId: 'owner' }, { operation: 'set_caps' }, r);
  if (!ownerOk(state, owner)) return res(DENIED);
  if (!caps || typeof caps !== 'object' || Array.isArray(caps) || Object.keys(caps).length === 0) return res(INVALID);
  for (const [k, v] of Object.entries(caps)) {
    // Atomic: validate everything first. A cap must be >= the safe default and <= the engine ceiling.
    if (!LIMIT_KEYS.includes(k) || !validLimit(v) || v < DEFAULT_LIMITS[k] || v > ENGINE_CEILINGS[k]) return res(INVALID);
  }
  const policy = (state.ownerPolicy ??= { version: 1, caps: {}, autoApply: { enabled: false } });
  policy.caps = { ...policy.caps, ...caps };
  policy.version += 1;
  return res({ ok: true, code: 'CAPS_SET', version: policy.version });
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
  if (request.objectCount !== undefined && !validLimit(request.objectCount)) return DENIED;
  const layer = KIND_LAYER[request.objectKind];
  if (!layer || !ELIGIBLE[layer].has(request.operation)) return DENIED;
  if (client.tier === 'cloud' && !CLOUD_LAYERS.has(layer)) return DENIED;
  if (state.collections?.[request.collectionId]?.layer !== layer) return DENIED;
  const grant = findGrant(state, client, auth, request);
  if (!grant) return DENIED;
  if (request.operation === 'create_import' && request.objectId !== undefined) return DENIED; // server assigns ids

  const digest = digestOf(request);
  if (!options.replay) {
    const eff = effectiveLimits(state, grant);
    if (!eff.ok) return DENIED;
    const { limits } = eff;
    if (Buffer.byteLength(request.payload) > limits.maxBytes) return LIMIT;
    if ((request.objectCount ?? 1) > limits.maxObjects) return LIMIT;
    if (recentWrites(state, grant.id) >= limits.maxWritesPerHour) return LIMIT;
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
export function ownerRestoreTombstone(state, owner, objectId) {
  const o = state?.objects?.[objectId];
  if (!ownerOk(state, owner) || !o || o.tombstoned !== true) return audit(state ?? {}, { clientId: 'owner' }, { operation: 'restore' }, DENIED);
  state.objects[objectId] = { ...o, tombstoned: false, revision: o.revision + 1, history: [...(o.history ?? []), snapshot(o)] };
  return audit(state, { clientId: 'owner' }, { operation: 'restore' }, R('RESTORED', true));
}

// ---- Local-tier bulk jobs: out-of-band, scoped, staged, reviewed by exception, applied separately ----
const jobKey = (auth, jobId) => auth.clientId + ':' + jobId;
const audienceCovers = (source, target) => Array.isArray(source) && Array.isArray(target) && target.every((r) => source.includes(r));
const stageGrant = (state, p, auth, job) => findGrant(state, p.client, auth, { collectionId: job.target, objectKind: job.kind, operation: 'bulk_stage' });
const applyGrant = (state, p, auth, job) => findGrant(state, p.client, auth, { collectionId: job.target, objectKind: job.kind, operation: 'bulk_apply' });

// A worker is ACTIVE while its renewable execution lease is unexpired. Lease expiry never touches staged proposals.
const activeWorkers = (state, clientId) => Object.values(state.jobs ?? {})
  .filter((j) => j.status === 'open' && state.now < j.leaseExpiresAt && (clientId === undefined || j.clientId === clientId)).length;

// Scope is the owner/prompt-declared project boundary; every source needs a CURRENT read grant. Ordinary raw -> curated promotion is
// the purpose of a bulk job (gated by tier + review); a PRIVATE source may only feed a target whose readers it also covers.
// Evaluated at job start, at lease renewal AND again inside the apply commit.
function sourceAuthorized(state, auth, targetId, collectionId) {
  const target = state.collections?.[targetId];
  const source = state.collections?.[collectionId];
  return !!target && !!source && canRead(state, auth, collectionId) &&
    !(source.private === true && !audienceCovers(source.audience, target.audience));
}
function scopeAuthorized(state, auth, targetId, scope) {
  return !!state.collections?.[targetId] && scope.every((c) => sourceAuthorized(state, auth, targetId, c));
}
const sameSet = (a, b) => Array.isArray(a) && Array.isArray(b) && a.length === b.length &&
  [...a].sort().every((x, i) => x === [...b].sort()[i]);
// Item-level provenance: every source and evidence collection an item cites must belong to the STAGING job's declared scope
// (the immutable scope recorded on the batch) and still be individually authorized for this principal and target.
const itemProvenanceAuthorized = (state, auth, batch, item) =>
  [...item.sources, ...item.evidence.map((e) => e.collectionId)]
    .every((c) => batch.scope.includes(c) && sourceAuthorized(state, auth, batch.target, c));

// Source evidence authority (coordinates with the MP-9C privacy contract without touching its files): a pointer must still
// resolve to the same content digest and must not have been forgotten. state.privacy.redacted holds "collection#ref" keys
// (or "collection#*" for a whole forgotten collection) and is owned by MP-9C; this model only reads it.
function evidenceAuthorized(state, items) {
  const redacted = state.privacy?.redacted ?? [];
  for (const item of items) {
    for (const e of item.evidence) {
      const key = e.collectionId + '#' + e.ref;
      if (redacted.includes(key) || redacted.includes(e.collectionId + '#*')) return DENIED;
    }
  }
  for (const item of items) {
    for (const e of item.evidence) {
      const src = state.sources?.[e.collectionId + '#' + e.ref];
      if (!src || src.digest !== e.digest) return STALE_EVIDENCE;
    }
  }
  return null;
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
  const eff = effectiveLimits(state, grant);
  if (!eff.ok) return res(DENIED);
  if (!scopeAuthorized(state, auth, job.targetCollectionId, job.scopeCollections)) return res(DENIED);
  const key = jobKey(auth, job.jobId);
  if (state.jobs?.[key]) return res(CONFLICT);
  // Worker concurrency: per-client effective limit (default 3, override <= owner cap) AND a vault-wide owner cap.
  if (activeWorkers(state, auth.clientId) >= eff.limits.maxConcurrentWorkers ||
      activeWorkers(state) >= eff.caps.maxConcurrentWorkers) return res(LIMIT);
  (state.jobs ??= {})[key] = {
    clientId: auth.clientId, epoch: state.epoch, policyRevision: p.client.writePolicyRevision,
    target: job.targetCollectionId, kind: job.objectKind, scope: [...job.scopeCollections],
    leaseExpiresAt: state.now + eff.limits.leaseMs, renewals: 0, itemCount: 0, status: 'open'
  };
  return res({ ok: true, code: 'STARTED', leaseExpiresAt: state.jobs[key].leaseExpiresAt });
}

function openJob(state, auth, jobId, { leaseRequired }) {
  const p = principal(state, auth);
  if (!p.ok || p.client.tier !== 'local') return null;
  const job = state.jobs?.[jobKey(auth, jobId)];
  if (!job || job.status !== 'open' || job.epoch !== state.epoch || job.policyRevision !== p.client.writePolicyRevision) return null;
  if (leaseRequired && state.now >= job.leaseExpiresAt) return null;
  return { p, job };
}

// The worker execution lease is renewable, but only while it is unexpired and the worker is still fully authorized.
// It is independent of the lifetime of staged proposals and of owner apply approvals.
export function renewWorkerLease(state, auth, jobId) {
  const res = (r) => audit(state ?? {}, auth, { operation: 'bulk_renew', collectionId: null }, r);
  const ctx = openJob(state, auth, jobId, { leaseRequired: true });
  if (!ctx) return res(DENIED);
  const { p, job } = ctx;
  const grant = stageGrant(state, p, auth, job);
  const eff = grant && effectiveLimits(state, grant);
  if (!eff?.ok || !scopeAuthorized(state, auth, job.target, job.scope)) return res(DENIED);
  job.leaseExpiresAt = state.now + eff.limits.leaseMs;
  job.renewals += 1;
  return res({ ok: true, code: 'RENEWED', leaseExpiresAt: job.leaseExpiresAt });
}

function itemEligible(state, p, auth, job, item) {
  if (!BULK_ITEM_OPERATIONS.has(item.operation) || typeof item.payload !== 'string') return DENIED;
  if (!findGrant(state, p.client, auth, { collectionId: job.target, objectKind: job.kind, operation: item.operation })) return DENIED;
  if (item.operation === 'create_import') return item.objectId === undefined ? null : DENIED;
  const o = state.objects?.[item.objectId];
  if (!o || o.vaultId !== state.vaultId || o.collectionId !== job.target || o.objectKind !== job.kind || o.tombstoned === true) return DENIED;
  return item.expectedRevision === o.revision ? null : CONFLICT;
}

// ---- deterministic risk classification and the owner review surface (C3) ----
// Risk is decided ONLY by these rules. Any model-supplied confidence, score or label is ignored and never part of a digest.
function classify(state, batch) {
  const targetSensitive = state.collections?.[batch.target]?.sensitive === true;
  const payloadsBySubject = new Map();
  for (const it of batch.items) {
    if (it.subject !== null) payloadsBySubject.set(it.subject, new Set([...(payloadsBySubject.get(it.subject) ?? []), sha256(it.payload)]));
  }
  return batch.items.map((it) => {
    const flags = [];
    const cols = [...it.sources, ...it.evidence.map((e) => e.collectionId)];
    if (cols.some((c) => state.collections?.[c]?.private === true)) flags.push('private_source');
    const existing = it.objectId === null ? undefined : state.objects?.[it.objectId];
    if (targetSensitive || existing?.sensitive === true) flags.push('sensitive_target');
    if (existing?.origin === 'owner' && it.operation !== 'create_import') flags.push('owner_claim');
    if (it.subject !== null && payloadsBySubject.get(it.subject).size > 1) flags.push('conflict');
    return { flags, risk: flags.length ? 'high' : it.operation === 'create_import' ? 'low' : 'medium' };
  });
}

// The digest binds the batch to the exact job that staged it (identity, target, kind and source scope), not only to its items.
const batchDigestOf = (batch) => sha256(canonical({
  items: batch.items, clusters: batch.clusters, jobKey: batch.jobKey, target: batch.target, kind: batch.kind,
  scope: [...batch.scope].sort(), riskPolicyVersion: RISK_POLICY_VERSION
}));
const countBy = (list, f) => list.reduce((m, x) => { const k = f(x); m[k] = (m[k] ?? 0) + 1; return m; }, {});

function autoApplyAllowed(state, client, clientId, batch, cls) {
  const policy = state.ownerPolicy?.autoApply;
  if (policy?.enabled !== true || !Array.isArray(policy.clients) || !policy.clients.includes(clientId) ||
      policy.maxRisk !== 'low' || !validLimit(policy.maxItems)) return false;
  const fake = { clientId };
  const grants = [applyGrant(state, { client }, fake, batch), ...[...new Set(batch.items.map((i) => i.operation))]
    .map((op) => findGrant(state, client, fake, { collectionId: batch.target, objectKind: batch.kind, operation: op }))];
  // Strictest policy wins: a single ask_first or missing grant forbids auto-apply.
  if (grants.some((g) => !g || g.mode !== 'allow')) return false;
  return batch.items.length <= policy.maxItems && cls.every((c) => c.risk === 'low');
}

// The approval surface contract. A pure, deterministic function of the immutable batch and current state; the owner approves
// exactly (batchDigest, summaryDigest, exception decisions). Evidence and full diff are expandable but never required.
export function buildReviewSurface(state, clientId, batchId) {
  const batch = state?.batches?.[clientId + ':' + batchId];
  const job = batch && state.jobs?.[batch.jobKey];
  const client = state?.clients?.[clientId];
  if (!batch || !job || !client) return null;
  const cls = classify(state, batch);
  const exceptions = cls.map((c, index) => ({ index, ...c })).filter((c) => c.risk === 'high')
    .map((c) => ({ index: c.index, flags: c.flags, operation: batch.items[c.index].operation, objectId: batch.items[c.index].objectId, cluster: batch.items[c.index].cluster, evidenceCount: batch.items[c.index].evidence.length }));
  const summary = {
    batchId, batchDigest: batchDigestOf(batch), riskPolicyVersion: RISK_POLICY_VERSION,
    jobKey: batch.jobKey, targetCollectionId: batch.target, scopeCollections: [...batch.scope],
    totalItems: batch.items.length, operations: countBy(batch.items, (i) => i.operation),
    risk: { low: 0, medium: 0, high: 0, ...countBy(cls, (c) => c.risk) },
    consolidatedItems: batch.items.filter((i) => i.evidence.length > 1).length,
    evidenceCount: batch.items.reduce((n, i) => n + i.evidence.length, 0),
    clusters: batch.clusters.map((c) => ({
      id: c.id, theme: c.theme, themeIsUntrustedText: true,
      items: batch.items.filter((i) => i.cluster === c.id).length,
      operations: countBy(batch.items.filter((i) => i.cluster === c.id), (i) => i.operation)
    })),
    exceptions
  };
  return {
    ...summary, summaryDigest: sha256(canonical(summary)),
    requiredDecisions: exceptions.map((e) => e.index),
    autoApplyEligible: autoApplyAllowed(state, client, clientId, batch, cls)
  };
}

export function reviewItemDetail(state, clientId, batchId, index) {
  const batch = state?.batches?.[clientId + ':' + batchId];
  const item = batch?.items?.[index];
  if (!item) return null;
  const cls = classify(state, batch)[index];
  const before = item.objectId === null ? null : state.objects?.[item.objectId]?.data ?? null;
  const after = item.operation === 'append' ? (before ?? '') + '\n' + item.payload : item.payload;
  return { index, operation: item.operation, objectId: item.objectId, cluster: item.cluster, risk: cls.risk, flags: cls.flags,
    evidence: item.evidence, sources: item.sources, before, after };
}
export const reviewFullDiff = (state, clientId, batchId) =>
  (state?.batches?.[clientId + ':' + batchId]?.items ?? []).map((_, i) => reviewItemDetail(state, clientId, batchId, i));

const proposalExpiry = (state, batch, grant) => {
  const eff = grant && effectiveLimits(state, grant);
  const ttl = eff?.ok ? eff.limits.proposalTtlMs : Math.min(DEFAULT_LIMITS.proposalTtlMs, ownerCaps(state).proposalTtlMs);
  // Tightening a TTL or cap takes effect on already staged proposals; loosening never resurrects beyond the original expiry.
  return Math.min(batch.expiresAt, batch.stagedAt + ttl);
};

export function stageBulkBatch(state, auth, jobId, batchId, items, meta = {}) {
  const res = (r) => audit(state ?? {}, auth, { operation: 'bulk_stage', collectionId: null }, r);
  const ctx = typeof batchId === 'string' && batchId && Array.isArray(items) && items.length ? openJob(state, auth, jobId, { leaseRequired: true }) : null;
  if (!ctx) return res(DENIED);
  const { p, job } = ctx;
  const grant = stageGrant(state, p, auth, job);
  const eff = grant && effectiveLimits(state, grant);
  if (!eff?.ok || !scopeAuthorized(state, auth, job.target, job.scope)) return res(DENIED);
  const { limits } = eff;
  if (items.length > limits.maxBatch || job.itemCount + items.length > limits.maxItems) return res(LIMIT);
  if (items.reduce((n, i) => n + Buffer.byteLength(typeof i.payload === 'string' ? i.payload : ''), 0) > limits.maxBytes) return res(LIMIT);
  const key = auth.clientId + ':' + batchId;
  if (state.batches?.[key]) return res(CONFLICT);
  const clusters = meta.clusters;
  if (!Array.isArray(clusters) || clusters.length === 0 || clusters.some((c) => typeof c?.id !== 'string' || !ID.test(c.id) || typeof c.theme !== 'string' || c.theme.length === 0 || c.theme.length > 200) ||
      new Set(clusters.map((c) => c.id)).size !== clusters.length) return res(INVALID);
  const seenObjects = new Set(); const seenPayloads = new Set();
  for (const item of items) {
    if (!Array.isArray(item.evidence) || item.evidence.length === 0 || !Array.isArray(item.sources) || item.sources.length === 0) return res(DENIED);
    if (typeof item.cluster !== 'string' || !clusters.some((c) => c.id === item.cluster)) return res(INVALID);
    if (item.evidence.some((e) => typeof e?.collectionId !== 'string' || typeof e.ref !== 'string' || typeof e.digest !== 'string' || !e.digest)) return res(INVALID);
    if (![...item.sources, ...item.evidence.map((e) => e.collectionId)].every((c) => job.scope.includes(c))) return res(DENIED);
    if (item.objectId !== undefined) { if (seenObjects.has(item.objectId)) return res(DENIED); seenObjects.add(item.objectId); }
    const bad = itemEligible(state, p, auth, job, item);
    if (bad) return res(bad);
    // Deterministic deduplication: identical payloads in one batch, or an update that changes nothing, are rejected.
    const h = sha256(item.payload);
    if (seenPayloads.has(h) || (item.operation === 'update' && state.objects[item.objectId].data === item.payload)) return res(DUPLICATE);
    seenPayloads.add(h);
  }
  const stale = evidenceAuthorized(state, items);
  if (stale) return res(stale);
  // Only deterministic fields are retained; confidence/score/label fields supplied by a model are dropped here.
  const normalized = items.map((i) => ({ operation: i.operation, objectId: i.objectId ?? null, expectedRevision: i.expectedRevision ?? null,
    payload: i.payload, evidence: i.evidence.map((e) => ({ collectionId: e.collectionId, ref: e.ref, digest: e.digest })),
    sources: [...i.sources], cluster: i.cluster, subject: i.subject ?? null }));
  const batch = { jobKey: jobKey(auth, jobId), target: job.target, kind: job.kind, scope: [...job.scope], items: normalized,
    clusters: clusters.map((c) => ({ id: c.id, theme: c.theme })), status: 'staged',
    stagedAt: state.now, expiresAt: state.now + limits.proposalTtlMs };
  batch.digest = batchDigestOf(batch);
  (state.batches ??= {})[key] = batch;
  job.itemCount += items.length;
  return res({ ok: true, code: 'STAGED', batchDigest: batch.digest, expiresAt: batch.expiresAt }); // staged items are NOT visible in state.objects
}

const decisionsDigest = (decisions) => sha256(canonical(decisions ?? {}));
const sameIndices = (decisions, indices) => {
  const a = Object.keys(decisions ?? {}).sort(); const b = indices.map(String).sort();
  return a.length === b.length && a.every((k, i) => k === b[i]);
};

// One short-lived, single-use approval bound to the exact displayed batch. The owner must have seen the CURRENT summary
// (summaryDigest) and decide every exception (confirm or withhold). Created only from the trusted local UI.
export function ownerApproveBatch(state, owner, clientId, batchId, { summaryDigest, decisions = {} } = {}) {
  const res = (r) => audit(state ?? {}, { clientId: 'owner' }, { operation: 'bulk_approve' }, r);
  const batch = state?.batches?.[clientId + ':' + batchId];
  if (!ownerOk(state, owner) || !batch || batch.status !== 'staged') return res(DENIED);
  const job = state.jobs?.[batch.jobKey];
  const client = state.clients?.[clientId];
  if (!job || !client || job.epoch !== state.epoch || job.policyRevision !== client.writePolicyRevision) return res(DENIED);
  const grant = findGrant(state, client, { clientId }, { collectionId: batch.target, objectKind: batch.kind, operation: 'bulk_apply' });
  if (!grant) return res(DENIED);
  const stage = findGrant(state, client, { clientId }, { collectionId: batch.target, objectKind: batch.kind, operation: 'bulk_stage' });
  if (state.now >= proposalExpiry(state, batch, stage)) return res(EXPIRED);
  const surface = buildReviewSurface(state, clientId, batchId);
  if (summaryDigest !== surface.summaryDigest) return res(STALE_SUMMARY); // the owner approved something other than what is shown now
  if (!sameIndices(decisions, surface.requiredDecisions) || !Object.values(decisions).every((d) => d === 'confirm' || d === 'withhold')) return res(INVALID);
  const eff = effectiveLimits(state, grant);
  if (!eff.ok) return res(DENIED);
  state.approvals.push({
    ownerApproved: true, clientId, vaultId: state.vaultId, collectionId: batch.target, operation: 'bulk_apply',
    operationId: batchId, bodyDigest: surface.batchDigest, summaryDigest: surface.summaryDigest,
    decisions: { ...decisions }, decisionsDigest: decisionsDigest(decisions), riskPolicyVersion: RISK_POLICY_VERSION,
    policyRevision: client.writePolicyRevision, epoch: state.epoch, expiresAt: state.now + eff.limits.approvalTtlMs, used: false
  });
  return res({ ok: true, code: 'APPROVED', expiresAt: state.now + eff.limits.approvalTtlMs });
}

// Owner-set deterministic auto-apply policy. Only low-risk batches may ever skip the owner approval.
export function setAutoApplyPolicy(state, owner, policy) {
  const res = (r) => audit(state ?? {}, { clientId: 'owner' }, { operation: 'set_auto_apply' }, r);
  if (!ownerOk(state, owner)) return res(DENIED);
  const caps = ownerCaps(state);
  if (typeof policy?.enabled !== 'boolean' || policy.maxRisk !== 'low' || !validLimit(policy.maxItems) || policy.maxItems > caps.maxBatch ||
      !Array.isArray(policy.clients) || policy.clients.some((c) => state.clients?.[c]?.tier !== 'local')) return res(INVALID);
  const current = (state.ownerPolicy ??= { version: 1, caps: {}, autoApply: { enabled: false } });
  current.autoApply = { enabled: policy.enabled, maxRisk: 'low', maxItems: policy.maxItems, clients: [...policy.clients] };
  current.version += 1;
  return res({ ok: true, code: 'AUTO_APPLY_SET', version: current.version });
}

export function applyBulkBatch(state, auth, jobId, batchId, options = {}) {
  const res = (r) => audit(state ?? {}, auth, { operation: 'bulk_apply', collectionId: null }, r);
  const early = openJob(state, auth, jobId, { leaseRequired: false }); // the WORKER lease is deliberately not required
  const first = early && state.batches?.[auth.clientId + ':' + batchId];
  if (!first || first.status !== 'staged') return res(DENIED);
  // Revalidated before and again inside the commit, with FRESH lookups each time: binding to the staging job, lifetime,
  // permissions, scope and item-level provenance, evidence authority/redaction, epoch, tombstones, expected revisions and
  // the approval route. Any failure applies nothing.
  const verify = () => {
    const pr = principal(state, auth);
    const job = state.jobs?.[jobKey(auth, jobId)];
    const batch = state.batches?.[auth.clientId + ':' + batchId];
    if (!pr.ok || pr.client.tier !== 'local' || !job || !batch || job.status !== 'open' || batch.status !== 'staged') return { err: DENIED };
    // A staged batch applies only through the job that staged it, and only while that job still carries the same immutable
    // target, kind and source scope the batch was staged under. Everything below uses the BATCH's target and scope.
    if (batch.jobKey !== jobKey(auth, jobId) || job.target !== batch.target || job.kind !== batch.kind || !sameSet(job.scope, batch.scope)) return { err: DENIED };
    if (job.epoch !== state.epoch || job.policyRevision !== pr.client.writePolicyRevision) return { err: DENIED };
    if (state.now >= proposalExpiry(state, batch, stageGrant(state, pr, auth, batch))) return { err: EXPIRED };
    if (!applyGrant(state, pr, auth, batch) || !scopeAuthorized(state, auth, batch.target, batch.scope)) return { err: DENIED };
    if (batchDigestOf(batch) !== batch.digest) return { err: DENIED }; // the staged batch was mutated after staging
    const surface = buildReviewSurface(state, auth.clientId, batchId);
    const approval = state.approvals?.find((a) =>
      a.used !== true && a.ownerApproved === true && a.operation === 'bulk_apply' && a.operationId === batchId &&
      a.clientId === auth.clientId && a.vaultId === state.vaultId && a.collectionId === batch.target &&
      a.bodyDigest === surface.batchDigest && a.summaryDigest === surface.summaryDigest &&
      a.decisionsDigest === decisionsDigest(a.decisions) && sameIndices(a.decisions, surface.requiredDecisions) &&
      a.riskPolicyVersion === RISK_POLICY_VERSION && a.policyRevision === pr.client.writePolicyRevision &&
      a.epoch === state.epoch && a.expiresAt > state.now) ?? null;
    // Strictest policy wins: autoApplyEligible already requires bulk_apply AND every contained operation to be `allow`, so a single
    // ask_first (or any non-low risk, disabled policy, unlisted client) routes the batch to the owner approval.
    const auto = surface.autoApplyEligible;
    if (!auto && !approval) return { err: APPROVAL_REQUIRED };
    const withheld = new Set(auto ? [] : Object.entries(approval.decisions).filter(([, d]) => d === 'withhold').map(([i]) => Number(i)));
    const live = batch.items.filter((_, i) => !withheld.has(i));
    if (!live.every((item) => itemProvenanceAuthorized(state, auth, batch, item))) return { err: DENIED };
    const staleOrRedacted = evidenceAuthorized(state, live);
    if (staleOrRedacted) return { err: staleOrRedacted };
    for (const item of live) {
      const bad = itemEligible(state, pr, auth, batch, { ...item, objectId: item.objectId ?? undefined, expectedRevision: item.expectedRevision ?? undefined });
      if (bad) return { err: bad };
    }
    return { ok: true, auto, approval, withheld, batch };
  };
  const stop = (v) => { if (v.err === EXPIRED) first.status = 'expired'; return res(v.err); };
  let v = verify();
  if (v.err) return stop(v);
  if (typeof options.beforeCommit === 'function') options.beforeCommit(state);
  v = verify(); // all-or-nothing: re-authorize everything inside the commit
  if (v.err) return stop(v);
  const { batch } = v; // the object verified inside the commit is the one applied
  if (v.approval) v.approval.used = true; // single-use, bound to this batch and decisions
  const created = []; const changed = [];
  batch.items.forEach((item, index) => {
    if (v.withheld.has(index)) return;
    if (item.operation === 'create_import') {
      const id = 'obj:' + auth.clientId + ':' + batchId + ':' + index;
      state.objects[id] = { vaultId: state.vaultId, collectionId: batch.target, objectKind: batch.kind, revision: 1, tombstoned: false, data: item.payload, history: [], batchId };
      created.push({ id, resultRevision: 1 });
    } else {
      const old = state.objects[item.objectId];
      const next = { ...old, revision: old.revision + 1, history: [...(old.history ?? []), snapshot(old)], batchId,
        data: item.operation === 'append' ? (old.data ?? '') + '\n' + item.payload : item.payload };
      state.objects[item.objectId] = next;
      changed.push({ id: item.objectId, before: snapshot(old), resultRevision: next.revision });
    }
  });
  batch.status = 'applied'; batch.applied = { created, changed, withheld: [...v.withheld] };
  return res({ ok: true, code: 'APPLIED', created: created.length, changed: changed.length, withheld: v.withheld.size, auto: v.auto });
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

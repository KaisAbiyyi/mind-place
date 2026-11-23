/**
 * Provider-neutral conformance claims C1..C9 against a live Mind Place listener,
 * executed by the SYNTHETIC HTTP client (wire.mjs). Nothing here knows a
 * provider's name. Real provider clients run their own scenarios (real.mjs) and
 * are judged on what the recording proxy saw; this suite is the server-side
 * reference those results are compared with.
 */
import crypto from 'node:crypto';
import { LEGACY_ERAS, MODERN } from './wire.mjs';

const TOOLS = ['memory_search', 'memory_fetch', 'context_build'];
const ZERO_SOURCE = `src:v1:${'0'.repeat(64)}`;

export class Claim {
  constructor(id) { this.id = id; this.checks = []; }
  check(name, ok, detail = '') { this.checks.push({ name, ok: Boolean(ok), detail: String(detail).slice(0, 300) }); return Boolean(ok); }
  get result() { return this.checks.length > 0 && this.checks.every((c) => c.ok) ? 'PASS' : 'FAIL'; }
  toJSON() { return { id: this.id, result: this.result, checks: this.checks }; }
}

const digest = (text) => crypto.createHash('sha256').update(text).digest('hex');

/**
 * Reassembles a revision from fetch pages and proves it is exact: contiguous byte ranges,
 * every message complete, text equal to the ground truth. Returns {ok, reason, pages, bytes}.
 */
export function verifyRevision(pages, expectedMessages, expected) {
  if (pages.length === 0) return { ok: false, reason: 'no pages' };
  const byMessage = new Map();
  let last = null;
  for (let i = 0; i < pages.length; i += 1) {
    const reply = pages[i];
    const result = reply.json?.result?.structuredContent;
    if (reply.status !== 200 || !result) return { ok: false, reason: `page ${i + 1} status ${reply.status}` };
    if (expected?.snapshot_id && result.snapshot_id !== expected.snapshot_id) return { ok: false, reason: `page ${i + 1} snapshot differs` };
    const isLast = i === pages.length - 1;
    if (!isLast && !(result.truncated === true && result.next_cursor)) return { ok: false, reason: `page ${i + 1} neither truncated nor last` };
    if (isLast && (result.truncated !== false || result.next_cursor)) return { ok: false, reason: 'last page still truncated or has a cursor (silent truncation)' };
    for (const seg of result.segments) {
      const entry = byMessage.get(seg.message_index) ?? { text: '', nextByte: 0, total: seg.message_total_bytes, id: seg.message_id, role: seg.role };
      if (seg.byte_start !== entry.nextByte) return { ok: false, reason: `message ${seg.message_index} gap or overlap at byte ${seg.byte_start} (expected ${entry.nextByte})` };
      entry.text += seg.text;
      entry.nextByte = seg.byte_end;
      byMessage.set(seg.message_index, entry);
      last = seg;
    }
  }
  const indices = [...byMessage.keys()].sort((a, b) => a - b);
  if (indices.length !== expectedMessages.length) return { ok: false, reason: `message count ${indices.length} != ${expectedMessages.length}` };
  let bytes = 0;
  for (let i = 0; i < indices.length; i += 1) {
    const entry = byMessage.get(indices[i]);
    if (indices[i] !== i) return { ok: false, reason: `message index ${i} missing` };
    if (entry.nextByte !== entry.total) return { ok: false, reason: `message ${i} incomplete (${entry.nextByte}/${entry.total} bytes)` };
    if (entry.text !== expectedMessages[i]) return { ok: false, reason: `message ${i} text differs from the immutable capture` };
    bytes += entry.total;
  }
  void last;
  return { ok: true, pages: pages.length, bytes, messages: indices.length, sha256: digest(indices.map((i) => byMessage.get(i).text).join('\u0000')) };
}

/** Schema keywords some strict clients reject. Informational: the real-client rows decide. */
/** True only for an https URL whose host is exactly chatgpt.com and whose path is a conversation path. */
export function isChatGptConversationUrl(value) {
  try {
    const url = new URL(String(value ?? ''));
    return url.protocol === 'https:' && url.hostname === 'chatgpt.com' && url.pathname.startsWith('/c/') && url.pathname.length > 3;
  } catch {
    return false;
  }
}

export function schemaFeatures(tools) {
  const found = new Set();
  const walk = (node) => {
    if (Array.isArray(node)) { node.forEach(walk); return; }
    if (!node || typeof node !== 'object') return;
    for (const [key, value] of Object.entries(node)) {
      if (['oneOf', 'anyOf', 'allOf', 'exclusiveMinimum', 'exclusiveMaximum', 'format', 'additionalProperties', 'minLength', 'maxLength'].includes(key)) found.add(key);
      if (key === 'type' && Array.isArray(value)) found.add('type-array');
      walk(value);
    }
  };
  tools.forEach((tool) => walk(tool.inputSchema));
  return [...found].sort();
}

/**
 * @param {{wire: ReturnType<import('./wire.mjs').createWire>, app: any, corpus: any, unlockSecret: string, log?: (s: string) => void}} ctx
 */
export async function runSyntheticSuite(ctx) {
  const { wire, app, corpus, unlockSecret } = ctx;
  const log = ctx.log ?? (() => {});
  const claims = Object.fromEntries(['C1', 'C2', 'C3', 'C4', 'C5', 'C6', 'C7', 'C8', 'C9'].map((id) => [id, new Claim(id)]));
  let progress = 'setup';
  try {
  const ref = (name, revision = 0) => {
    const rev = corpus[name].revisions[revision];
    return { archive_id: rev.archive_id, snapshot_id: rev.snapshot_id, source_id: ctx.sourceIds[corpus[name].title] };
  };
  const fetchArgs = (r) => ({ source_id: r.source_id, archive_id: r.archive_id, snapshot_id: r.snapshot_id, granularity: 'revision' });
  const outcome = (reply) => `${reply.status}:${JSON.stringify(reply.json?.error ?? null)}`;
  const hitsOf = (reply) => reply.json?.result?.structuredContent?.hits ?? [];

  // Two read credentials: A will read the long and medium chats, B only the second one.
  const A = await app.issueReader('Conformance client A');
  const B = await app.issueReader('Conformance client B');
  const C = await app.issueReader('Conformance client C (never granted)');
  ctx.secrets?.push(A.credential, B.credential, C.credential);
  const rows = await app.grantableChats(A.clientId);
  ctx.sourceIds = Object.fromEntries(rows.map((row) => [row.title, row.source_id]));
  log('credentials issued');

  // C1 handshake and initialization.
  progress = 'C1';
  {
    const c = claims.C1;
    for (const era of LEGACY_ERAS) {
      const hs = await wire.handshake(A.credential, era);
      c.check(`initialize ${era} negotiates that revision`, hs.init.status === 200 && hs.init.json?.result?.protocolVersion === era, outcome(hs.init));
      c.check(`initialized notification ${era} is accepted`, hs.initialized.status === 202, hs.initialized.status);
      c.check(`serverInfo names the product (${era})`, hs.init.json?.result?.serverInfo?.name === 'mind-place', JSON.stringify(hs.init.json?.result?.serverInfo));
    }
    const modern = await wire.handshake(A.credential, MODERN);
    c.check('modern discovery lists its revision', modern.discover.status === 200 && modern.discover.json?.result?.supportedVersions?.includes(MODERN), outcome(modern.discover));
    const anon = await wire.post(null, 'initialize', { protocolVersion: LEGACY_ERAS[1], capabilities: {} }, { era: LEGACY_ERAS[1] });
    c.check('initialize without a credential is refused', anon.status === 401, outcome(anon));
    const junk = await wire.post('mpc_v1_' + '0'.repeat(64), 'initialize', { protocolVersion: LEGACY_ERAS[1], capabilities: {} }, { era: LEGACY_ERAS[1] });
    c.check('initialize with an unknown key is refused', junk.status === 401, outcome(junk));
    const bad = await wire.post(A.credential, 'tools/list', {}, { era: '2025-01-01' });
    c.check('an unsupported protocol revision is refused', bad.status === 400 && bad.json?.error?.code === -32022, outcome(bad));
  }

  progress = 'C2';
  // C2 tool discovery: exactly the read tools, flagged read-only, nothing writable reachable.
  {
    const c = claims.C2;
    let features = [];
    for (const era of [MODERN, ...LEGACY_ERAS]) {
      const list = await wire.toolsList(A.credential, era);
      const tools = list.json?.result?.tools ?? [];
      c.check(`tools/list (${era}) returns exactly the three read tools`, JSON.stringify(tools.map((t) => t.name)) === JSON.stringify(TOOLS), tools.map((t) => t.name).join(','));
      c.check(`every tool is annotated read-only and non-destructive (${era})`, tools.every((t) => t.annotations?.readOnlyHint === true && t.annotations?.destructiveHint === false), '');
      features = schemaFeatures(tools);
    }
    for (const name of ['memory_write', 'memory_delete', 'memory_update', 'archive_write', 'issue_key', 'shell']) {
      const reply = await wire.call(A.credential, name, {}, MODERN);
      c.check(`unknown or write tool "${name}" is not callable`, reply.status === 404, outcome(reply));
    }
    ctx.schemaFeatures = features;
    c.check('schema keywords strict clients may reject are recorded (informational)', true, features.join(','));
  }

  progress = 'C6a';
  // C6 (before any grant): a fresh credential reads nothing, and a guess looks like an unknown id.
  {
    const c = claims.C6;
    for (const [label, token] of [['A', A.credential], ['B', B.credential], ['C', C.credential]]) {
      for (const word of ['sentinel-long-5', 'sentinel-medium-3', 'sentinel-second-2', 'sentinel-canary-2']) {
        const reply = await wire.search(token, [word]);
        c.check(`default deny: client ${label} finds nothing for ${word} before a grant`, reply.status === 200 && reply.hits.length === 0, `${reply.status}:${reply.hits.length}`);
      }
    }
    const unknown = await wire.call(A.credential, 'memory_fetch', { ...fetchArgs(ref('medium')), source_id: ZERO_SOURCE }, MODERN);
    for (const name of ['long', 'medium', 'second', 'other']) {
      const reply = await wire.call(A.credential, 'memory_fetch', fetchArgs(ref(name)), MODERN);
      c.check(`ungranted fetch of "${name}" is indistinguishable from an unknown id`, outcome(reply) === outcome(unknown), `${outcome(reply)} vs ${outcome(unknown)}`);
    }
  }

  progress = 'grants';
  // Owner grants: A reads long + medium, B reads second, C stays ungranted.
  await app.grantSources(A.clientId, [ctx.sourceIds[corpus.long.title], ctx.sourceIds[corpus.medium.title]]);
  await app.grantSources(B.clientId, [ctx.sourceIds[corpus.second.title]]);
  log('grants applied');

  progress = 'C3';
  // C3 scoped search.
  {
    const c = claims.C3;
    const own = await wire.search(A.credential, ['sentinel-long-5']);
    c.check('A finds its granted long chat', own.status === 200 && own.hits.length > 0 && own.hits.every((h) => h.conversation_id === corpus.long.revisions[0].archive_id || h.conversation_id.endsWith('aaaaaaaaaaa1')), `${own.status}:${own.hits.length}`);
    const med = await wire.search(A.credential, ['sentinel-medium-3']);
    c.check('A finds its granted medium chat', med.hits.length > 0 && med.hits.every((h) => h.conversation_id.endsWith('bbbbbbbbbbb2')), `${med.hits.length}`);
    for (const word of ['sentinel-second-2', 'sentinel-canary-2']) {
      const reply = await wire.search(A.credential, [word]);
      c.check(`A never sees ungranted content (${word})`, reply.hits.length === 0, `${reply.hits.length}`);
    }
    const wide = await wire.search(A.credential, ['sentinel'], { limit: 100 });
    c.check('a broad query returns only granted chats', wide.hits.length > 0 && wide.hits.every((h) => /aaaaaaaaaaa1$|bbbbbbbbbbb2$/.test(h.conversation_id)), [...new Set(wide.hits.map((h) => h.conversation_id.slice(-4)))].join(','));
    const forced = await wire.search(A.credential, ['sentinel'], { filters: { source_ids: [ctx.sourceIds[corpus.other.title]] } });
    c.check('a filter naming an ungranted source does not widen access', forced.hits.length === 0, `${forced.hits.length}`);
    const bOwn = await wire.search(B.credential, ['sentinel-second-2']);
    c.check('B finds only its own chat', bOwn.hits.length > 0 && bOwn.hits.every((h) => h.conversation_id.endsWith('ccccccccccc3')), `${bOwn.hits.length}`);
    const bCross = await wire.search(B.credential, ['sentinel-long-5']);
    c.check("cross-client isolation: B cannot find A's chat", bCross.hits.length === 0, `${bCross.hits.length}`);
    // Real clients issue parallel tool calls (Codex does). They must be served, not refused with 503.
    for (const era of [MODERN, '2025-06-18']) {
      const burst = await Promise.all(Array.from({ length: 6 }, () => wire.search(A.credential, ['sentinel-long-5'], {}, era)));
      c.check(`six parallel searches from one client are all served (${era})`, burst.every((r) => r.status === 200 && r.hits.length > 0), burst.map((r) => r.status).join(','));
    }
    const ctxA = await wire.call(A.credential, 'context_build', { queries: ['sentinel-canary-2'] }, MODERN);
    c.check('context_build does not assemble ungranted evidence', ctxA.status === 200 && !JSON.stringify(ctxA.json).includes('sentinel-canary'), outcome(ctxA));
    const ctxOwn = await wire.call(A.credential, 'context_build', { queries: ['sentinel-long-5'] }, MODERN);
    c.check('context_build works on granted evidence', ctxOwn.status === 200 && JSON.stringify(ctxOwn.json).includes('sentinel-long-5'), outcome(ctxOwn));
  }

  progress = 'C4';
  // C4 exact full-content fetch, long conversation, multi-page, both protocol eras.
  const exact = {};
  {
    const c = claims.C4;
    const head = corpus.long.revisions.at(-1);
    for (const era of [MODERN, '2025-06-18']) {
      for (const maxBytes of [64 * 1024, 9000]) {
        const pages = await wire.fetchRevision(A.credential, ref('long', 2), { maxBytes, era });
        const verdict = verifyRevision(pages, head.messages, head);
        c.check(`long chat latest revision is byte-exact across ${verdict.pages ?? pages.length} pages (${era}, ${maxBytes} B pages)`, verdict.ok && (maxBytes > 9000 || verdict.pages > 1), verdict.reason ?? `${verdict.pages} pages, ${verdict.bytes} bytes`);
        exact[`${era}:${maxBytes}`] = verdict.sha256;
      }
    }
    c.check('page size and protocol era do not change the content (same digest)', new Set(Object.values(exact)).size === 1, JSON.stringify(exact).slice(0, 200));
    const med = corpus.medium.revisions.at(-1);
    const medPages = await wire.fetchRevision(A.credential, ref('medium', 1), { maxBytes: 4096 });
    const medVerdict = verifyRevision(medPages, med.messages, med);
    c.check(`medium chat is exact over ${medVerdict.pages ?? medPages.length} small pages`, medVerdict.ok && medVerdict.pages >= 5, medVerdict.reason ?? medVerdict.pages);
    c.check('the long chat really is larger than one default page', (head.messages.reduce((n, m) => n + Buffer.byteLength(m), 0)) > 256 * 1024, `${head.messages.length} messages`);
    const anyMultiByte = head.messages.some((m) => /[^\u0000-ÿ]/.test(m));
    c.check('the corpus contains multi-byte and astral-plane text', anyMultiByte, '');
  }

  progress = 'C5';
  // C5 revisions, identity and provenance.
  {
    const c = claims.C5;
    const sources = new Set();
    for (let i = 0; i < corpus.long.revisions.length; i += 1) {
      const rev = corpus.long.revisions[i];
      const pages = await wire.fetchRevision(A.credential, ref('long', i), { maxBytes: 64 * 1024 });
      const verdict = verifyRevision(pages, rev.messages, rev);
      c.check(`historical revision ${i + 1} (${rev.message_count} messages) is still exact after later revisions`, verdict.ok, verdict.reason ?? '');
      const first = pages[0]?.json?.result?.structuredContent;
      sources.add(first?.source_id);
      c.check(`revision ${i + 1} carries stable identity and provenance`, first?.snapshot_id === rev.snapshot_id && first?.provider === 'chatgpt' && /^\d{4}-\d\d-\d\dT/.test(first?.captured_at ?? '') && first?.title === corpus.long.title && isChatGptConversationUrl(first?.source_url), JSON.stringify({ s: first?.snapshot_id === rev.snapshot_id, p: first?.provider, t: first?.captured_at }));
      const seg = first?.segments?.[0];
      c.check(`revision ${i + 1} segments carry message id, index, role and byte range`, Boolean(seg?.message_id) && Number.isInteger(seg?.message_index) && Boolean(seg?.role) && Number.isInteger(seg?.byte_end), JSON.stringify(seg ? { id: seg.message_id, i: seg.message_index, r: seg.role } : null));
    }
    c.check('the source id is stable across all revisions', sources.size === 1, [...sources].join(','));
    c.check('revision 3 differs from revision 2 only where the edit was made (history is immutable)', corpus.long.revisions[1].messages[120] !== corpus.long.revisions[2].messages[120] && corpus.long.revisions[1].messages[119] === corpus.long.revisions[2].messages[119], '');
    const hist = await wire.search(A.credential, ['sentinel-long-3'], { filters: { include_history: true }, limit: 100 });
    const snaps = new Set(hist.hits.map((h) => h.snapshot_id));
    c.check('search with history sees more than one revision of the same chat', snaps.size > 1, `${snaps.size} snapshot(s)`);
    for (const hit of hist.hits.slice(0, 3)) c.check('a search hit carries source, archive, snapshot and message ids, provider and timestamp', Boolean(hit.source_id && hit.archive_id && hit.snapshot_id && hit.message_id && hit.provider && hit.captured_at), '');
  }

  progress = 'C6b';
  // C6 (after grants): isolation and denied reads.
  {
    const c = claims.C6;
    const unknown = await wire.call(A.credential, 'memory_fetch', { ...fetchArgs(ref('long', 2)), source_id: ZERO_SOURCE }, MODERN);
    for (const name of ['second', 'other']) {
      const reply = await wire.call(A.credential, 'memory_fetch', fetchArgs(ref(name)), MODERN);
      c.check(`A: granted-set fetch of ungranted "${name}" looks like an unknown id`, outcome(reply) === outcome(unknown), `${outcome(reply)} vs ${outcome(unknown)}`);
    }
    for (const name of ['long', 'medium', 'other']) {
      const reply = await wire.call(B.credential, 'memory_fetch', fetchArgs(ref(name)), MODERN);
      c.check(`B cannot read "${name}" even with exact ids`, outcome(reply) === outcome(unknown), outcome(reply));
    }
    const noGrant = await wire.call(C.credential, 'memory_fetch', fetchArgs(ref('long', 2)), MODERN);
    c.check('C (never granted) cannot read anything', outcome(noGrant) === outcome(unknown), outcome(noGrant));
    const swapped = await wire.call(B.credential, 'memory_fetch', fetchArgs(ref('second')), MODERN);
    c.check("B's own chat is readable (the denials are about scope, not a broken client)", swapped.status === 200, outcome(swapped));
  }

  progress = 'C7';
  // C7 immediate revocation: chat grant, then the whole client.
  {
    const c = claims.C7;
    await app.revokeSources(A.clientId, [ctx.sourceIds[corpus.medium.title]]);
    const medium = await wire.call(A.credential, 'memory_fetch', fetchArgs(ref('medium', 1)), MODERN);
    const mediumSearch = await wire.search(A.credential, ['sentinel-medium-3']);
    const stillLong = await wire.search(A.credential, ['sentinel-long-5']);
    c.check('revoking one chat takes effect on the very next fetch', medium.status !== 200, outcome(medium));
    c.check('revoking one chat takes effect on the very next search', mediumSearch.hits.length === 0, `${mediumSearch.hits.length}`);
    c.check('other grants of the same client are unaffected', stillLong.hits.length > 0, `${stillLong.hits.length}`);
    await app.revokeClient(B.clientId);
    const dead = await wire.toolsList(B.credential, MODERN);
    const deadSearch = await wire.search(B.credential, ['sentinel-second-2']);
    c.check('revoking a client denies its key on the next request', [401, 403].includes(dead.status), outcome(dead));
    c.check('a revoked client reads nothing', deadSearch.hits.length === 0 && deadSearch.status !== 200, `${deadSearch.status}`);
    const aStill = await wire.search(A.credential, ['sentinel-long-5']);
    c.check("revoking B does not disturb A (no cross-client effect)", aStill.hits.length > 0, `${aStill.hits.length}`);
  }

  // Normalise A's grants for the Lock and reconnect claims: long only.
  const grantedNow = await app.grantedSources(A.clientId);
  const dropNow = grantedNow.filter((id) => id !== ctx.sourceIds[corpus.long.title]);
  if (dropNow.length > 0) await app.revokeSources(A.clientId, dropNow);
  const grantedBefore = (await app.grantedSources(A.clientId)).slice().sort();

  progress = 'C8';
  // C8 Lock.
  {
    const c = claims.C8;
    const before = await wire.search(A.credential, ['sentinel-long-5']);
    c.check('before Lock, A reads its chat', before.hits.length > 0, `${before.hits.length}`);
    await app.lock();
    await new Promise((resolve) => setTimeout(resolve, 1500));
    let status = 'closed';
    let leaked = false;
    try {
      const reply = await wire.search(A.credential, ['sentinel-long-5']);
      status = String(reply.status);
      leaked = reply.hits.length > 0;
    } catch { status = 'closed'; }
    c.check('after Lock the listener refuses or is closed', ['closed', '401', '403'].includes(status), status);
    c.check('after Lock nothing is disclosed', !leaked, '');
    let listStatus = 'closed';
    try { listStatus = String((await wire.toolsList(A.credential, MODERN)).status); } catch { listStatus = 'closed'; }
    c.check('after Lock even tool discovery is unavailable', ['closed', '401', '403'].includes(listStatus), listStatus);
  }

  progress = 'C9';
  // C9 reconnect without escalation: unlock, start sharing again, same key, same grants.
  {
    const c = claims.C9;
    await app.unlock(unlockSecret);
    await app.startSharing();
    const regained = await wire.search(A.credential, ['sentinel-long-5']);
    c.check('after unlock and restart the same key works again for its grant', regained.status === 200 && regained.hits.length > 0, `${regained.status}:${regained.hits.length}`);
    const grantedAfter = (await app.grantedSources(A.clientId)).slice().sort();
    c.check('the grant set is exactly what it was before Lock (no escalation)', JSON.stringify(grantedAfter) === JSON.stringify(grantedBefore), `${grantedBefore.length} -> ${grantedAfter.length}`);
    for (const word of ['sentinel-medium-3', 'sentinel-second-2', 'sentinel-canary-2']) {
      const reply = await wire.search(A.credential, [word]);
      c.check(`still no access to ungranted content after reconnect (${word})`, reply.hits.length === 0, `${reply.hits.length}`);
    }
    const revoked = await wire.toolsList(B.credential, MODERN);
    c.check('a revoked client stays revoked after Lock and unlock', [401, 403].includes(revoked.status), outcome(revoked));
    const fresh = await wire.search(C.credential, ['sentinel']);
    c.check('a never-granted key still reads nothing after reconnect', fresh.hits.length === 0, `${fresh.hits.length}`);
    const hs = await wire.handshake(A.credential, LEGACY_ERAS[1]);
    c.check('a new handshake succeeds after reconnect', hs.init.status === 200, outcome(hs.init));
  }

  progress = 'done';
  } catch (error) {
    // Keep every claim result gathered so far; the failure is reported, not hidden.
    const reason = `harness stopped during ${progress}: ${String(error?.message ?? error).split(/\r?\n/)[0]}`;
    for (const claim of Object.values(claims)) {
      if (claim.checks.length === 0) claim.check('claim could not run', false, reason);
    }
    ctx.harnessFailure = reason;
  }
  return { claims: Object.fromEntries(Object.entries(claims).map(([k, v]) => [k, v.toJSON()])), schemaFeatures: ctx.schemaFeatures ?? [], harnessFailure: ctx.harnessFailure ?? null };
}

/**
 * Real-client scenario. One provider client (see clients.mjs) is run, several times with small prompts,
 * against the packaged app behind the recording proxy. The owner-side steps (grant, revoke, Lock,
 * unlock) are driven through the same owner commands the Connections screen uses.
 *
 * Verdicts come from the proxy log, never from what the model said: the client announced its own
 * name and version, which tools/call requests it really sent, and what the server really answered.
 */
import crypto from 'node:crypto';
import fs from 'node:fs';
import { Claim, verifyRevision } from './suite.mjs';
import { DRIVERS, makeWorkDir } from './clients.mjs';
import { safePreview, sanitizeBlocked, scrubDiagnostic } from './hygiene.mjs';
import { observedClientIdentity, startRecordingProxy } from './recording-proxy.mjs';
import { createWire, MODERN } from './wire.mjs';

const SERVER = 'mpconformance';
const ONLY = `Use ONLY the MCP server named ${SERVER}. Do not use any other tool, server, shell or file. `;

const ok = (e) => e.status === 200 && e.result && !e.error;
const calls = (exs, tool) => exs.filter((e) => e.rpcMethod === 'tools/call' && e.toolName === tool);
const hitsOf = (e) => e.result?.structuredContent?.hits ?? [];
const asReply = (e) => ({ status: e.status, json: e.error ? { error: e.error } : { result: e.result } });
const errKey = (e) => `${e.status}:${JSON.stringify(e.error ?? null)}`;

/**
 * The fetch chain a client really executed for one snapshot, in order. A call the server rejected (for
 * example the model's own malformed first attempt) is not part of the chain: the client retries, and what
 * matters is that the accepted calls follow the server's cursors and add up to the exact revision.
 */
function chainFor(exs, snapshotId) {
  return calls(exs, 'memory_fetch').filter((e) => e.params?.arguments?.snapshot_id === snapshotId && e.status === 200 && e.result);
}

const rejectedFetches = (exs, snapshotId) => calls(exs, 'memory_fetch').filter((e) => e.params?.arguments?.snapshot_id === snapshotId && e.status !== 200).length;

/** Key order is not meaning: compare cursors structurally. */
function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  if (value && typeof value === 'object') return `{${Object.keys(value).sort().map((k) => `${JSON.stringify(k)}:${canonical(value[k])}`).join(',')}}`;
  return JSON.stringify(value ?? null);
}

/** Short digest of canonical arguments: lets a receipt compare argument shapes without carrying any id. */
export const argsDigest = (value) => crypto.createHash('sha256').update(canonical(value)).digest('hex').slice(0, 12);

/** Short digest of the COMPLETE canonical arguments object (including cursor, budget, message_id, unknown fields). */
export const fullArgsDigest = (args) => argsDigest(args ?? {});

/** The arguments that decide which revision is read; paging and budget do not. */
export const identityArgs = (args) => ({ source_id: args?.source_id ?? null, archive_id: args?.archive_id ?? null, snapshot_id: args?.snapshot_id ?? null, granularity: args?.granularity ?? null });

/** Classify whether a client's memory_fetch arguments match the canonical request completely or only on revision identity. */
export function classifyFetchArgsMatch(clientArgs, canonicalValidArgs) {
  const clientFullDigest = fullArgsDigest(clientArgs ?? {});
  const canonicalFullDigest = fullArgsDigest(canonicalValidArgs ?? {});
  const clientIdentityDigest = argsDigest(identityArgs(clientArgs));
  const canonicalIdentityDigest = argsDigest(identityArgs(canonicalValidArgs));
  const completeArgsMatch = clientFullDigest === canonicalFullDigest;
  const identityMatches = clientIdentityDigest === canonicalIdentityDigest;
  return {
    identityMatches,
    completeArgsMatch,
    clientFullDigest,
    canonicalFullDigest,
    identityDigest: canonicalIdentityDigest,
    mode: completeArgsMatch ? 'complete_args_equal' : identityMatches ? 'identity_only_match' : 'mismatch',
  };
}

function chainHonest(chain) {
  for (let i = 0; i < chain.length; i += 1) {
    const sent = chain[i].params?.arguments?.cursor ?? null;
    const expected = i === 0 ? null : chain[i - 1].result?.structuredContent?.next_cursor ?? null;
    if (canonical(sent) !== canonical(expected)) return `call ${i + 1} sent a cursor that is not the server's previous next_cursor`;
  }
  return null;
}

export async function runRealClient(clientId, { startSession, log = () => {} }) {
  const driver = DRIVERS[clientId];
  if (!driver) throw new Error(`Unknown client ${clientId}`);
  const found = driver.detect();
  if (!found) return { clientId, blocked: { kind: 'NOT_INSTALLED', detail: `${driver.label} is not installed on this machine` } };

  const session = await startSession({ label: `real-${clientId}`, corpusOptions: { longMessages: 6, longBytes: 500, mediumMessages: 10, mediumBytes: 1400 }, log });
  const { app, corpus, secrets, unlockSecret } = session;
  const proxy = await startRecordingProxy({ secrets });
  const wire = createWire();
  const claims = Object.fromEntries(['C1', 'C2', 'C3', 'C4', 'C5', 'C6', 'C7', 'C8', 'C9'].map((id) => [id, new Claim(id)]));
  const transcript = [];
  let identity = null;
  let progress = 'setup';
  let notConnected = null;
  let c4Verdict = null;
  let permissionProof = null;
  try {
    const K = await app.issueReader('Real client');
    const K2 = await app.issueReader('Real client (client-revoke test)');
    secrets.push(K.credential, K2.credential);
    const rows = await app.grantableChats(K.clientId);
    const sid = Object.fromEntries(rows.map((r) => [r.title, r.source_id]));
    const med = corpus.medium;
    const [rev1, rev2] = med.revisions;
    const ids = (rev) => `source_id "${sid[med.title]}", archive_id "${rev.archive_id}", snapshot_id "${rev.snapshot_id}"`;

    const exercise = async (label, token, prompt, timeoutMs = 420000) => {
      const mark = proxy.mark();
      const workDir = makeWorkDir(clientId);
      try {
        const out = await driver.run({ url: proxy.url, token, prompt, workDir, timeoutMs, secrets });
        const exs = proxy.since(mark);
        transcript.push({ label, exit: out.exit, serverStatus: out.serverStatus ?? null, discoveredTools: out.discoveredTools ?? null, exchanges: exs.length });
        log(`${label}: exit ${out.exit}, ${exs.length} exchanges`);
        return { out, exs };
      } finally {
        try {
          fs.rmSync(workDir, { recursive: true, force: true, maxRetries: 10, retryDelay: 250 });
        } catch {
          // Best-effort cleanup on Windows when a child bridge process releases the temp dir briefly after exit;
          // workDir contains only non-secret config files (tokens are passed via env only).
        }
      }
    };
    const unknownFetch = async (rev) => outcomeOf(await wire.call(K.credential, 'memory_fetch', { source_id: `src:v1:${'0'.repeat(64)}`, archive_id: rev.archive_id, snapshot_id: rev.snapshot_id, granularity: 'revision' }, MODERN));
    const outcomeOf = (r) => `${r.status}:${JSON.stringify(r.json?.error ?? null)}`;
    // A denial only means something if the very same arguments are valid when access is granted.
    // We record both the COMPLETE canonical request digest (all argument fields) and the revision
    // identity digest (excluding cursor/budget/extra fields), and explicitly report which matched.
    const validArgs = { source_id: sid[med.title], archive_id: rev2.archive_id, snapshot_id: rev2.snapshot_id, granularity: 'revision' };
    const canonicalFullDigest = fullArgsDigest(validArgs);
    const identityOnlyDigest = argsDigest(identityArgs(validArgs));
    const findMatchingFetch = (exs) => {
      const fetches = calls(exs, 'memory_fetch');
      return fetches.find((e) => classifyFetchArgsMatch(e.params?.arguments, validArgs).completeArgsMatch)
        ?? fetches.find((e) => classifyFetchArgsMatch(e.params?.arguments, validArgs).identityMatches)
        ?? null;
    };
    const probe = async () => wire.call(K.credential, 'memory_fetch', validArgs, MODERN);
    const canonicalArgsPrompt = JSON.stringify(validArgs);

    // R0 default deny: nothing is granted yet.
    progress = 'R0';
    const unknownOutcome = await unknownFetch(rev2);
    const r0 = await exercise('R0 default deny', K.credential,
      `${ONLY}Step 1: call memory_search with {"queries": ["sentinel-medium-3"]}. Step 2: call memory_fetch with exact arguments ${canonicalArgsPrompt} and no extra fields. Reply with one line: the hit count and whether the fetch succeeded. Do not retry.`);
    identity = observedClientIdentity(proxy.exchanges);
    const handshaken = r0.exs.some((e) => e.status === 200);
    if (r0.exs.length === 0 || !handshaken) {
      // The client never reached the server: nothing can be concluded about Mind Place, so do not report FAIL cells.
      notConnected = sanitizeBlocked({
        reason: r0.exs.length === 0 ? 'no request reached the listener' : `every request was refused (${[...new Set(r0.exs.map((e) => e.status))].join(',')}); the client did not deliver a valid credential`,
        requestsSeen: r0.exs.length,
        exit: r0.out.exit,
        serverStatus: r0.out.serverStatus ?? null,
        stderr: r0.out.stderr ?? '',
        stdoutTail: r0.out.stdoutTail ?? '',
        received: proxy.received.length,
      }, secrets);
      throw new Error('CLIENT_DID_NOT_CONNECT');
    }
    const deniedBeforeGrant = outcomeOf(await probe());
    const r0Fetch = findMatchingFetch(r0.exs);
    const r0Match = r0Fetch ? classifyFetchArgsMatch(r0Fetch.params?.arguments, validArgs) : null;
    {
      const c = claims.C6;
      const search = calls(r0.exs, 'memory_search')[0];
      c.check('the real client searched before any grant and got nothing', Boolean(search) && ok(search) && hitsOf(search).length === 0, search ? `${search.status}:${hitsOf(search).length}` : 'client sent no search');
      c.check('before any grant the harness, with the proven-valid canonical fetch arguments, is denied exactly like an unknown id', deniedBeforeGrant === unknownOutcome, `canonical_full ${canonicalFullDigest}: ${deniedBeforeGrant} vs ${unknownOutcome}`);
      c.check('the real client fetched with valid arguments before any grant and was denied like an unknown id', Boolean(r0Fetch) && errKey(r0Fetch) === unknownOutcome, r0Fetch ? `${r0Match.mode} (client_full ${r0Match.clientFullDigest} vs canonical_full ${canonicalFullDigest}, identity ${identityOnlyDigest}): ${errKey(r0Fetch)}` : `client sent no fetch matching canonical_full ${canonicalFullDigest} or identity ${identityOnlyDigest}`);
    }

    // Owner grants the medium chat to K.
    await app.grantSources(K.clientId, [sid[med.title]]);
    const grantedProbe = await probe();
    const isRevision200 = (reply) => reply.status === 200 && !reply.json?.error && reply.json?.result?.structuredContent?.snapshot_id === rev2.snapshot_id && (reply.json?.result?.structuredContent?.segments?.length ?? 0) > 0;
    let r0ReplayedOk = isRevision200(grantedProbe);
    if (r0Fetch && r0Match?.mode === 'identity_only_match') {
      const replayReply = await wire.call(K.credential, 'memory_fetch', r0Fetch.params.arguments, MODERN);
      r0ReplayedOk = r0ReplayedOk && isRevision200(replayReply);
    }
    claims.C6.check('the same arguments are valid once granted: HTTP 200 with the revision content (so the denial was permission, not malformed input)', r0ReplayedOk, `${r0Match?.mode ?? 'canonical'} (canonical_full ${canonicalFullDigest}, client_full ${r0Match?.clientFullDigest ?? 'n/a'}): ${outcomeOf(grantedProbe)}`);

    // R1 handshake, discovery, scoped search.
    progress = 'R1';
    const r1 = await exercise('R1 scoped search', K.credential,
      `${ONLY}Call memory_search three times, once each with queries ["sentinel-medium-3"], ["sentinel-canary-2"], ["sentinel-second-2"]. Reply with the three hit counts only.`);
    identity = observedClientIdentity(proxy.exchanges) ?? identity;
    {
      const c = claims.C1;
      const init = r1.exs.find((e) => e.rpcMethod === 'initialize') ?? proxy.exchanges.find((e) => e.rpcMethod === 'initialize');
      const discover = proxy.exchanges.find((e) => e.rpcMethod === 'server/discover');
      c.check('the client announced its own name and version on the wire', Boolean(identity?.name && identity?.version), JSON.stringify(identity));
      c.check('the server accepted the client handshake', Boolean((init && init.status === 200) || (discover && discover.status === 200)), init ? `initialize ${init.status}` : discover ? `discover ${discover.status}` : 'no handshake seen');
      const negotiated = init?.result?.protocolVersion ?? null;
      c.check('a protocol revision was negotiated', Boolean(negotiated) || Boolean(discover), String(negotiated));
    }
    {
      const c = claims.C2;
      const list = proxy.exchanges.find((e) => e.rpcMethod === 'tools/list');
      const names = (list?.result?.tools ?? []).map((t) => t.name);
      c.check('the client requested tools/list and received the three read-only tools', list?.status === 200 && JSON.stringify(names) === JSON.stringify(['memory_search', 'memory_fetch', 'context_build']), names.join(','));
      c.check('the client went on to call a discovered tool (it accepted the tool schemas)', calls(r1.exs, 'memory_search').length > 0, `${calls(r1.exs, 'memory_search').length} search call(s)`);
      if (r1.out.discoveredTools) c.check('the client itself lists the three tools as discovered', ['memory_search', 'memory_fetch', 'context_build'].every((t) => r1.out.discoveredTools.includes(t)), r1.out.discoveredTools.join(','));
    }
    {
      const c = claims.C3;
      const searches = calls(r1.exs, 'memory_search');
      const bySentinel = (word) => searches.find((e) => JSON.stringify(e.params?.arguments?.queries ?? '').includes(word));
      const a = bySentinel('sentinel-medium-3');
      const b = bySentinel('sentinel-canary-2');
      const d = bySentinel('sentinel-second-2');
      c.check('the real client found its granted chat', Boolean(a) && ok(a) && hitsOf(a).length > 0 && hitsOf(a).every((h) => h.conversation_id.endsWith('bbbbbbbbbbb2')), a ? `${hitsOf(a).length} hits` : 'no search sent');
      c.check('the real client got nothing for the never-granted canary chat', Boolean(b) && ok(b) && hitsOf(b).length === 0, b ? `${hitsOf(b).length} hits` : 'no search sent');
      c.check('the real client got nothing for the other collection', Boolean(d) && ok(d) && hitsOf(d).length === 0, d ? `${hitsOf(d).length} hits` : 'no search sent');
      c.check('the real client never received an ungranted conversation id in any response', ![...searches].some((e) => JSON.stringify(e.result ?? '').includes('cccccccccc') || JSON.stringify(e.result ?? '').includes('ddddddddddd4')), '');
    }
    {
      const c = claims.C6;
      const b = calls(r1.exs, 'memory_search').find((e) => JSON.stringify(e.params?.arguments?.queries ?? '').includes('sentinel-canary-2'));
      c.check('after the grant the canary and second collections stay invisible to the real client', Boolean(b) && hitsOf(b).length === 0, '');
    }

    // R2 exact full-content read, both revisions, small pages so the client really pages.
    progress = 'R2';
    const r2 = await exercise('R2 paged exact read', K.credential,
      `${ONLY}Read two revisions of one conversation completely with memory_fetch, never printing the text. Revision A: ${ids(rev2)}. Revision B: ${ids(rev1)}. Always use granularity "revision" and budget {"max_bytes": 3000}. After every call, if next_cursor is not null, call memory_fetch again with the same arguments plus cursor set to EXACTLY the next_cursor value just returned; stop when next_cursor is null. Do revision A first, then revision B. Reply with the number of calls made for each.`,
      900000);
    for (const [label, rev] of [['latest', rev2], ['earlier', rev1]]) {
      const chain = chainFor(r2.exs, rev.snapshot_id);
      const verdict = verifyRevision(chain.map(asReply), rev.messages, rev);
      const dishonest = chainHonest(chain);
      if (label === 'latest') c4Verdict = verdict;
      const target = label === 'latest' ? claims.C4 : claims.C5;
      target.check(`the real client read the ${label} revision exactly (${verdict.pages ?? chain.length} pages, ${verdict.bytes ?? 0} bytes)`, verdict.ok && !dishonest && (label === 'earlier' || verdict.pages >= 5), verdict.reason ?? dishonest ?? `${verdict.pages} pages`);
      const rejected = rejectedFetches(r2.exs, rev.snapshot_id);
      if (label === 'latest') claims.C4.check('the client followed the server cursors and sent nothing else (no fabricated cursor)', dishonest === null, (dishonest ?? 'every accepted call used the previous next_cursor') + (rejected ? `; ${rejected} call(s) were rejected by the server (client-side invalid arguments) and retried` : ''));
      const first = chain[0]?.result?.structuredContent;
      claims.C5.check(`the ${label} revision came back with stable identity and provenance`, first?.snapshot_id === rev.snapshot_id && first?.provider === 'chatgpt' && /^\d{4}-/.test(first?.captured_at ?? '') && first?.source_id === sid[med.title], JSON.stringify({ p: first?.provider, s: first?.source_id === sid[med.title] }));
    }
    claims.C5.check('both revisions of the same chat were read through one source id', chainFor(r2.exs, rev1.snapshot_id).length > 0 && chainFor(r2.exs, rev2.snapshot_id).length > 0, '');

    // R3 immediate revocation of the chat grant.
    progress = 'R3';
    await app.revokeSources(K.clientId, [sid[med.title]]);
    const r3 = await exercise('R3 after chat revoke', K.credential,
      `${ONLY}Step 1: call memory_search with {"queries": ["sentinel-medium-3"]}. Step 2: call memory_fetch with exact arguments ${canonicalArgsPrompt} and no extra fields. Reply with the hit count and whether the fetch succeeded. Do not retry.`);
    {
      const c = claims.C7;
      const s = calls(r3.exs, 'memory_search')[0];
      const f = findMatchingFetch(r3.exs);
      const r3Match = f ? classifyFetchArgsMatch(f.params?.arguments, validArgs) : null;
      const deniedAfterRevoke = outcomeOf(await probe());
      c.check('the very next search by the real client returns nothing after the chat was revoked', Boolean(s) && ok(s) && hitsOf(s).length === 0, s ? `${hitsOf(s).length}` : 'no search');
      c.check('after the revoke the canonical arguments that were accepted (HTTP 200) while granted are denied exactly like an unknown id', deniedAfterRevoke === unknownOutcome, `canonical_full ${canonicalFullDigest}: ${deniedAfterRevoke} vs ${unknownOutcome}`);
      c.check('the very next fetch by the real client, with valid arguments, is denied like an unknown id', Boolean(f) && errKey(f) === unknownOutcome, f ? `${r3Match.mode} (client_full ${r3Match.clientFullDigest} vs canonical_full ${canonicalFullDigest}, identity ${identityOnlyDigest}): ${errKey(f)}` : `client sent no fetch matching canonical_full ${canonicalFullDigest} or identity ${identityOnlyDigest}`);
      permissionProof = {
        canonical_full_digest: canonicalFullDigest,
        identity_digest: identityOnlyDigest,
        valid_granted_200: r0ReplayedOk,
        r0: r0Match ? { mode: r0Match.mode, client_full_digest: r0Match.clientFullDigest, replayed_while_granted_200: r0ReplayedOk } : null,
        r3: r3Match ? { mode: r3Match.mode, client_full_digest: r3Match.clientFullDigest } : null,
      };
    }
    // Whole-client revoke, with a second credential.
    progress = 'R3b';
    await app.grantSources(K2.clientId, [sid[med.title]]);
    const r3a = await exercise('R3b client works before revoke', K2.credential, `${ONLY}Call memory_search with queries ["sentinel-medium-3"] and reply with the hit count only.`);
    await app.revokeClient(K2.clientId);
    const r3b = await exercise('R3b client after client revoke', K2.credential, `${ONLY}Call memory_search with queries ["sentinel-medium-3"] and reply with the hit count only.`);
    {
      const c = claims.C7;
      const before = calls(r3a.exs, 'memory_search')[0];
      c.check('a second real-client credential worked before it was revoked', Boolean(before) && hitsOf(before).length > 0, before ? `${hitsOf(before).length} hits` : 'no search');
      const refused = r3b.exs.filter((e) => e.status === 401 || e.status === 403);
      const gotResult = r3b.exs.some((e) => e.rpcMethod === 'tools/call' && ok(e));
      c.check('after the whole client was revoked the real client was refused and read nothing', refused.length > 0 && !gotResult, `${refused.length} refusals, results=${gotResult}`);
    }

    // R4 Lock.
    progress = 'R4';
    await app.grantSources(K.clientId, [sid[med.title]]);
    await app.lock();
    await new Promise((resolve) => setTimeout(resolve, 1500));
    const r4 = await exercise('R4 after Lock', K.credential, `${ONLY}Call memory_search with queries ["sentinel-medium-3"] and reply with the hit count only. If the server is unreachable, say so.`);
    {
      const c = claims.C8;
      const disclosed = r4.exs.some((e) => ok(e) && (e.rpcMethod === 'tools/call'));
      const unreachable = r4.exs.some((e) => e.upstreamError || [401, 403].includes(e.status));
      c.check('after Lock the real client received no tool result', !disclosed, `${r4.exs.length} exchanges`);
      c.check('after Lock the real client tried to connect and the listener was unreachable or refused every request', unreachable && r4.exs.every((e) => e.upstreamError || [401, 403].includes(e.status)), r4.exs.length === 0 ? 'the client made no attempt, so nothing is proven' : r4.exs.map((e) => e.upstreamError ? 'unreachable' : e.status).join(','));
    }

    // R5 reconnect.
    progress = 'R5';
    await app.unlock(unlockSecret);
    await app.startSharing();
    const grantedAfter = await app.grantedSources(K.clientId);
    const r5 = await exercise('R5 after reconnect', K.credential,
      `${ONLY}Call memory_search twice, once with queries ["sentinel-medium-3"] and once with ["sentinel-canary-2"]. Reply with the two hit counts only.`);
    {
      const c = claims.C9;
      const a = calls(r5.exs, 'memory_search').find((e) => JSON.stringify(e.params?.arguments?.queries ?? '').includes('sentinel-medium-3'));
      const b = calls(r5.exs, 'memory_search').find((e) => JSON.stringify(e.params?.arguments?.queries ?? '').includes('sentinel-canary-2'));
      c.check('with the same key and configuration the real client reconnected and reads its grant', Boolean(a) && ok(a) && hitsOf(a).length > 0, a ? `${hitsOf(a).length} hits` : 'no search');
      c.check('after reconnecting it still cannot read the canary chat (no escalation)', Boolean(b) && ok(b) && hitsOf(b).length === 0, b ? `${hitsOf(b).length}` : 'no search');
      c.check('the grant set after reconnect is exactly the one before Lock', grantedAfter.length === 1 && grantedAfter[0] === sid[med.title], `${grantedAfter.length} grant(s)`);
      const refusedRevoked = await wire.toolsList(K2.credential, MODERN);
      c.check('the revoked client stayed revoked', [401, 403].includes(refusedRevoked.status), String(refusedRevoked.status));
    }
    progress = 'done';
  } catch (error) {
    if (notConnected) { /* reported below as blocked */ } else {
    const reason = scrubDiagnostic(`harness stopped during ${progress}: ${String(error?.message ?? error)}`, secrets).split(/\r?\n/)[0];
    for (const claim of Object.values(claims)) if (claim.checks.length === 0) claim.check('claim could not run', false, reason);
    transcript.push({ label: 'harness', failure: reason });
    }
  } finally {
    await proxy.close();
    await app.close();
  }
  identity = identity ?? observedClientIdentity(proxy.exchanges);
  if (notConnected) return { clientId, label: driver.label, installed: found, blocked: sanitizeBlocked({ kind: 'CLIENT_COULD_NOT_USE_THE_SERVER', detail: 'The real client was run with a task that needs the MCP server but never completed a handshake. Nothing can be concluded about Mind Place from this run.', ...notConnected }, secrets), secrets };
  return {
    clientId,
    label: driver.label,
    installed: found,
    observed_client: identity,
    permission_proof: permissionProof,
    // What this real client actually read for C4. The long (over 256 KB) chat is verified by the synthetic row only.
    c4_scope: `Medium multi-byte two-revision chat: latest revision ${c4Verdict?.bytes ?? 'n/a'} bytes read in ${c4Verdict?.pages ?? 'n/a'} pages of at most 3000 bytes, earlier revision also read. The long chat over 256 KB was NOT read by this real client (NOT_RUN for real clients; covered by the synthetic reference only).`,
    transcript,
    claims: Object.fromEntries(Object.entries(claims).map(([k, v]) => [k, v.toJSON()])),
    exchange_summary: proxy.exchanges.map((e) => ({
      at: e.at, http: e.http, rpc: e.rpcMethod, tool: e.toolName, status: e.status, upstreamError: e.upstreamError ?? undefined,
      hits: e.result?.structuredContent?.hits ? e.result.structuredContent.hits.length : undefined,
      segments: e.result?.structuredContent?.segments ? e.result.structuredContent.segments.length : undefined,
      error: e.error ? { code: e.error.code, message: safePreview(e.error.message, secrets, 160) } : undefined,
      truncated: e.result?.structuredContent?.truncated, bytes: e.responseBytes, snapshot: e.params?.arguments?.snapshot_id ? String(e.params.arguments.snapshot_id).slice(0, 12) : undefined,
    })),
    sourceKey: session.exe,
    secrets,
  };
}

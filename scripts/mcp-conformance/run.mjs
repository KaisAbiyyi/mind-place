/**
 * Release 1 read-only MCP conformance runner.
 *
 *   node scripts/mcp-conformance/run.mjs synthetic            # server reference (synthetic HTTP client)
 *   node scripts/mcp-conformance/run.mjs real claude-code     # a real provider client behind the recording proxy
 *   node scripts/mcp-conformance/run.mjs matrix               # merge receipts into the Parity Matrix
 *
 * Needs the packaged app (npm run build -> release/win32-x64) and the MCP port free.
 * Everything runs against a disposable synthetic archive and a disposable unlock
 * secret. Keys never touch a command line, URL, log, receipt or screenshot.
 */
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { findSecretsOnDisk, scrubText } from '../lib/e2e-secret-guard.mjs';
import { launchApp, portIsBusy } from './app-driver.mjs';
import { seedCorpus } from './corpus.mjs';
import { formatSafeJson, formatSafeLine, safePreview, sanitizeBlocked } from './hygiene.mjs';
import { runSyntheticSuite } from './suite.mjs';
import { createWire } from './wire.mjs';
import { runRealClient } from './real.mjs';
import { startRecordingProxy } from './recording-proxy.mjs';
import { CLAIMS, CLIENTS, STORE_MODE, renderMatrixMarkdown, validateMatrix } from './matrix.mjs';

export const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const git = (...args) => spawnSync('git', args, { cwd: repoRoot, encoding: 'utf8' }).stdout.trim();

export function sourceIdentity() {
  const sha = git('rev-parse', 'HEAD');
  const dirty = git('status', '--porcelain', '--untracked-files=no').length > 0;
  return {
    sha,
    dirty,
    // The binary is built from Rust sources and the frontend. Binding both lets a reader see exactly what the executable embodies.
    trees: {
      crates: git('rev-parse', 'HEAD:crates'),
      src_tauri: git('rev-parse', 'HEAD:src-tauri'),
      cargo_manifest: git('rev-parse', 'HEAD:Cargo.toml'),
      cargo_lock: git('rev-parse', 'HEAD:Cargo.lock'),
      src: git('rev-parse', 'HEAD:src'),
      extension: git('rev-parse', 'HEAD:extension'),
      frontend_lock: git('rev-parse', 'HEAD:package-lock.json'),
    },
    // The conformance harness (including the stdio bridge) that produced the evidence. Recorded separately from the product so both are verifiable.
    harness: {
      conformance: git('rev-parse', 'HEAD:scripts/mcp-conformance'),
      lib: git('rev-parse', 'HEAD:scripts/lib'),
      stdio_bridge: git('rev-parse', 'HEAD:scripts/mcp-stdio-bridge.mjs'),
    },
  };
}

export function productPaths() {
  const dir = path.join(repoRoot, 'release', `${process.platform}-${process.arch}`);
  return { exe: path.join(dir, 'mind-place.exe'), host: path.join(dir, 'mind-place-capture-host.exe') };
}

export function sha256File(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
}

const VAULT_DATABASES = ['archive-index.sqlite3', 'state.sqlite3', 'index.sqlite3'];

export function verifyVaultFiles(runtime, legacyPath) {
  assert.ok(!fs.existsSync(legacyPath), 'a legacy archive appeared during the vault run');
  for (const name of VAULT_DATABASES) {
    const fd = fs.openSync(path.join(runtime, 'encrypted-vault-v1', name), 'r');
    try {
      const header = Buffer.alloc(16);
      assert.equal(fs.readSync(fd, header), header.length, `${name} has no database header`);
      assert.ok(!header.equals(Buffer.from('SQLite format 3\0')), `${name} has a plaintext SQLite header`);
    } finally {
      fs.closeSync(fd);
    }
  }
}

/** Provision the vault before the first capture, then verify the owner read for every fixture revision. */
export async function startSession({ label, corpusOptions, log = () => {} }) {
  const { exe, host } = productPaths();
  if (!fs.existsSync(exe) || !fs.existsSync(host)) throw new Error('Missing packaged app: run npm run build first');
  if (await portIsBusy(43731)) throw new Error('MCP port 43731 is in use: close any running Mind Place first');
  const base = path.resolve(process.env.MP_CONFORMANCE_DIR ?? os.tmpdir());
  fs.mkdirSync(base, { recursive: true });
  const runtime = fs.mkdtempSync(path.join(base, `${label}-`));
  const dataDir = path.join(runtime, 'archive-v1');
  const unlockSecret = crypto.randomBytes(24).toString('hex');
  const secrets = [unlockSecret];
  let app;
  const close = async () => {
    try {
      await app?.close();
    } finally {
      fs.rmSync(runtime, { recursive: true, force: true, maxRetries: 10, retryDelay: 250 });
    }
  };
  try {
    app = await launchApp({ exe, runtimeDir: runtime, dataDir });
    await app.provisionVault(unlockSecret, secrets);
    const corpus = seedCorpus({ host, archiveRoot: dataDir, ...corpusOptions });
    await app.invoke('refresh_archive_index');
    verifyVaultFiles(runtime, dataDir);
    for (const { revisions } of Object.values(corpus)) {
      for (const revision of revisions) {
        const ownerRead = await app.invoke('read_conversation', { id: revision.archive_id, snapshotId: revision.snapshot_id });
        assert.equal(ownerRead.snapshot_id, revision.snapshot_id, 'owner read returned the wrong revision');
        const digest = crypto.createHash('sha256').update(JSON.stringify(ownerRead.capture.messages.map((message) => message.text))).digest('hex');
        assert.equal(digest, revision.expected_messages_sha256, 'owner read differs from the captured fixture');
      }
    }
    await app.startSharing();
    log(`encrypted vault unlocked, ${Object.keys(corpus).length} conversations verified, sharing on`);
    return { app, close, corpus, unlockSecret, secrets, exe, storeMode: STORE_MODE };
  } catch (error) {
    await close().catch(() => {});
    throw new Error(safePreview(String(error?.message ?? error), secrets, 400));
  }
}

export function writeReceipt(dir, name, body, secrets) {
  fs.mkdirSync(dir, { recursive: true });
  const text = scrubText(JSON.stringify(body, null, 2), secrets) + '\n';
  const file = path.join(dir, name);
  fs.writeFileSync(file, text);
  const leaks = findSecretsOnDisk(dir, secrets).filter((p) => p.startsWith(name));
  if (leaks.length) { fs.rmSync(file, { force: true }); throw new Error(`Receipt refused: ${leaks.join('; ')}`); }
  return { file, sha256: crypto.createHash('sha256').update(text).digest('hex') };
}

async function syntheticPhase() {
  const source = sourceIdentity();
  const outDir = path.join(repoRoot, 'outputs', 'mcp-conformance', source.sha.slice(0, 12));
  let activeSecrets = [];
  const log = (m) => console.error(formatSafeLine(`[synthetic] ${m}`, activeSecrets));
  const session = await startSession({ label: 'syn', log });
  const { app, corpus, unlockSecret, secrets } = session;
  activeSecrets = secrets;
  let result;
  let failure = null;
  try {
    result = await runSyntheticSuite({ wire: createWire(), app, corpus, unlockSecret, secrets, log });
  } catch (error) {
    failure = formatSafeLine(String(error?.message ?? error), secrets);
  } finally {
    await session.close();
  }
  const body = {
    kind: 'SYNTHETIC_HTTP reference run against the packaged app (disposable synthetic corpus). Not proof of any provider client.',
    source,
    executable_sha256: sha256File(session.exe),
    store_mode: session.storeMode,
    dirty_tree: source.dirty,
    page_errors: app.pageErrors.map((e) => safePreview(e, secrets, 250)),
    corpus_summary: Object.fromEntries(Object.entries(corpus).map(([k, v]) => [k, v.revisions.map((r) => ({ messages: r.message_count, expected_messages_sha256: r.expected_messages_sha256 }))])),
    schema_features: result?.schemaFeatures ?? [],
    c4_scope: 'Long chat (over 256 KB, 3 revisions, multi-byte, astral-plane and right-to-left text): latest revision at 64 KB and 9 KB pages in two protocol eras and every earlier revision; medium chat in 4 KB pages. A server reference, not a provider client.',
    harness_failure: result?.harnessFailure ? formatSafeLine(result.harnessFailure, secrets) : null,
    failure,
    claims: result?.claims ?? null,
  };
  const receipt = writeReceipt(outDir, 'synthetic.json', body, secrets);
  const summary = Object.fromEntries(Object.entries(result?.claims ?? {}).map(([k, v]) => [k, v.result]));
  console.log(formatSafeJson({ receipt: receipt.file, sha256: receipt.sha256, summary, failure }, secrets));
  for (const claim of Object.values(result?.claims ?? {})) {
    for (const check of claim.checks.filter((c) => !c.ok)) console.log(formatSafeLine(`FAIL ${claim.id} ${check.name} :: ${check.detail}`, secrets));
  }
  process.exitCode = failure || result?.harnessFailure || Object.values(summary).some((r) => r !== 'PASS') ? 1 : 0;
}

/**
 * Merge the receipts of ONE exact SHA into the Parity Matrix. Cells without a receipt must be explicit
 * (NOT_RUN or PROVIDER_BLOCKED) in static-cells.json; nothing is ever inferred or defaulted to PASS.
 */
const treeId = (trees) => crypto.createHash('sha256').update(JSON.stringify(trees ?? {})).digest('hex').slice(0, 16);

async function matrixPhase() {
  const source = sourceIdentity();
  const outDir = path.join(repoRoot, 'outputs', 'mcp-conformance', source.sha.slice(0, 12));
  const docsDir = path.join(repoRoot, 'docs', 'qualification', 'mcp-parity');
  const receiptsDir = path.join(docsDir, 'receipts');
  fs.mkdirSync(receiptsDir, { recursive: true });
  const statics = JSON.parse(fs.readFileSync(path.join(docsDir, 'static-cells.json'), 'utf8'));
  const problems = [];
  if (source.dirty) problems.push('working tree has uncommitted changes: evidence must come from an exact committed SHA');
  const cells = {};
  for (const client of CLIENTS) cells[client.id] = {};
  const fileFor = (client) => (client.synthetic ? 'synthetic.json' : `real-${client.id}.json`);
  for (const client of CLIENTS) {
    const src = path.join(outDir, fileFor(client));
    const have = fs.existsSync(src);
    const blockedSrc = path.join(outDir, `blocked-${client.id}.json`);
    if (fs.existsSync(blockedSrc)) fs.copyFileSync(blockedSrc, path.join(receiptsDir, `blocked-${client.id}.json`));
    let receipt = null;
    let rel = null;
    let hash = null;
    if (have) {
      receipt = JSON.parse(fs.readFileSync(src, 'utf8'));
      if (!/^[0-9a-f]{40}$/.test(receipt.source?.sha ?? '')) problems.push(`${client.id}: receipt has no exact SHA`);
      if (treeId(receipt.source?.trees) !== treeId(source.trees)) problems.push(`${client.id}: receipt was produced from a different product tree than HEAD (the executable must match the sources)`);
      if (receipt.dirty_tree) problems.push(`${client.id}: receipt was produced from a dirty tree`);
      if (receipt.source?.sha !== source.sha) problems.push(`${client.id}: receipt ran at ${String(receipt.source?.sha).slice(0, 9)}, not at ${source.sha.slice(0, 9)}: rerun every client at one frozen commit`);
      if (treeId(receipt.source?.harness) !== treeId(source.harness)) problems.push(`${client.id}: receipt was produced by a different conformance harness than HEAD`);
      if (receipt.store_mode !== STORE_MODE) problems.push(`${client.id}: receipt did not verify the encrypted vault`);
      const text = fs.readFileSync(src);
      fs.writeFileSync(path.join(receiptsDir, fileFor(client)), text);
      rel = `receipts/${fileFor(client)}`;
      hash = crypto.createHash('sha256').update(text).digest('hex');
    }
    for (const claim of CLAIMS) {
      const fromReceipt = receipt?.claims?.[claim.id];
      const fixed = statics[client.id]?.[claim.id] ?? statics[client.id]?._all;
      if (fromReceipt && fromReceipt.checks?.length) {
        cells[client.id][claim.id] = {
          result: fromReceipt.result,
          sha: receipt.source.sha,
          product_tree_id: treeId(receipt.source.trees),
          harness_tree_id: treeId(receipt.source.harness),
          store_mode: receipt.store_mode,
          ...(claim.id === 'C4' && receipt.c4_scope ? { scope: receipt.c4_scope } : {}),
          evidence: client.synthetic
            ? { kind: 'SYNTHETIC_HTTP', receipt: rel, receipt_sha256: hash, transport: client.transport }
            : { kind: 'REAL_CLIENT', receipt: rel, receipt_sha256: hash, transport: client.transport, observed_client: receipt.observed_client ?? undefined },
          note: `${fromReceipt.checks.filter((c) => c.ok).length}/${fromReceipt.checks.length} checks${fromReceipt.result === 'FAIL' ? '; failing: ' + fromReceipt.checks.filter((c) => !c.ok).map((c) => c.name).join(' | ').slice(0, 260) : ''}`,
          executable_sha256: receipt.executable_sha256,
          client_version: receipt.installed?.version ?? undefined,
        };
        if (!client.synthetic && cells[client.id][claim.id].evidence.observed_client === undefined) cells[client.id][claim.id].evidence.observed_client = { name: '', version: '' };
      } else if (fixed) {
        cells[client.id][claim.id] = { ...fixed };
      } else {
        problems.push(`${client.id}/${claim.id}: no receipt and no explicit static cell`);
      }
    }
  }
  const matrix = { sha: source.sha, product_tree_id: treeId(source.trees), harness_tree_id: treeId(source.harness), store_mode: STORE_MODE, generated_at: new Date().toISOString(), executable_sha256: JSON.parse(fs.readFileSync(path.join(outDir, 'synthetic.json'), 'utf8')).executable_sha256, product_trees: source.trees, cells };
  problems.push(...validateMatrix(matrix));
  fs.writeFileSync(path.join(docsDir, 'matrix.json'), JSON.stringify(matrix, null, 2) + '\n');
  fs.writeFileSync(path.join(docsDir, 'PARITY-MATRIX.md'), renderMatrixMarkdown(matrix));
  const leaks = findSecretsOnDisk(docsDir, []);
  problems.push(...leaks);
  console.log(formatSafeJson({ sha: source.sha, problems }, []));
  process.exitCode = problems.length ? 1 : 0;
}

/**
 * Owner test card helper: a disposable, unlocked Mind Place with the synthetic conformance corpus, left open
 * for the owner to connect a client that cannot be driven from here (hosted ChatGPT, Gemini app, Claude app).
 * Pass `--proxy` to expose a local recording proxy in front of 127.0.0.1:43731 and emit a scrubbed wire receipt.
 * The owner creates the access keys and picks chats in the real Connections screen. Nothing private is loaded.
 */
async function servePhase(useProxy = false) {
  const source = sourceIdentity();
  const outDir = path.join(repoRoot, 'outputs', 'mcp-conformance', source.sha.slice(0, 12));
  let activeSecrets = [];
  const log = (m) => console.error(formatSafeLine(`[serve] ${m}`, activeSecrets));
  const session = await startSession({ label: 'serve', corpusOptions: { longMessages: 6, longBytes: 500, mediumMessages: 10, mediumBytes: 1400 }, log });
  activeSecrets = session.secrets;
  const proxy = useProxy ? await startRecordingProxy({ targetPort: 43731, secrets: session.secrets }) : null;
  console.log('Disposable Mind Place is running with a synthetic corpus (4 chats). It is unlocked and sharing is on.');
  if (proxy) console.log(`Recording proxy listening at ${proxy.url} (forwards to http://127.0.0.1:43731/mcp; writes scrubbed wire receipt on exit).`);
  console.log('In the app: open Connections -> create Key 1 ("<client>-main") with ZERO chats granted first for P0 (default deny).');
  console.log('Only AFTER P0 passes, Choose chats -> allow ONLY "Conformance medium conversation". Use a SEPARATE Key 2 for P4 whole-client revoke.');
  console.log('Follow docs/qualification/mcp-parity/OWNER-TEST-CARDS.md. Press Ctrl-C here when done (writes scrubbed session receipt and discards the archive).');
  let stopping = false;
  const stop = async () => {
    if (stopping) return;
    stopping = true;
    const receiptBody = {
      kind: 'OWNER_SERVE_SESSION_RECEIPT',
      source,
      executable_sha256: sha256File(session.exe),
      store_mode: session.storeMode,
      dirty_tree: source.dirty,
      proxy_used: Boolean(proxy),
      observed_client: proxy ? (await import('./recording-proxy.mjs')).observedClientIdentity(proxy.exchanges) : null,
      exchange_summary: proxy ? proxy.exchanges.map((e) => ({
        at: e.at, http: e.http, rpc: e.rpcMethod, tool: e.toolName, status: e.status, upstreamError: e.upstreamError ?? undefined,
        hits: e.result?.structuredContent?.hits ? e.result.structuredContent.hits.length : undefined,
        segments: e.result?.structuredContent?.segments ? e.result.structuredContent.segments.length : undefined,
        error: e.error ? { code: e.error.code, message: safePreview(e.error.message, session.secrets, 160) } : undefined,
        bytes: e.responseBytes,
      })) : [],
      contains_no_keys_or_private_ids: true,
    };
    const stamp = new Date().toISOString().replace(/[:.]/g, '-');
    const saved = writeReceipt(outDir, `owner-serve-${stamp}.json`, receiptBody, session.secrets);
    console.log(formatSafeJson({ owner_serve_receipt: saved.file, sha256: saved.sha256 }, session.secrets));
    if (proxy) await proxy.close();
    await session.close();
    process.exit(0);
  };
  process.on('SIGINT', stop);
  await new Promise(() => {});
}

async function realPhase(clientId) {
  const source = sourceIdentity();
  const outDir = path.join(repoRoot, 'outputs', 'mcp-conformance', source.sha.slice(0, 12));
  const activeSecrets = [];
  const log = (m) => console.error(formatSafeLine(`[real:${clientId}] ${m}`, activeSecrets));
  const result = await runRealClient(clientId, {
    startSession: async (opts) => {
      const s = await startSession(opts);
      activeSecrets.push(...s.secrets);
      return s;
    },
    log,
  });
  const { secrets = [], ...rest } = result;
  activeSecrets.push(...secrets);
  if (rest.blocked) {
    const safeBlocked = sanitizeBlocked(rest.blocked, activeSecrets);
    const blockedReceipt = writeReceipt(outDir, `blocked-${clientId}.json`, { source, clientId, installed: rest.installed, blocked: safeBlocked }, activeSecrets);
    console.log(formatSafeJson({ clientId, blocked: safeBlocked, receipt: blockedReceipt.file }, activeSecrets));
    process.exitCode = 3;
    return;
  }
  const body = {
    kind: 'REAL_CLIENT run: the actual provider client behind a recording proxy against the packaged app (disposable synthetic corpus). Verdicts come from the wire log, not the model transcript.',
    source,
    executable_sha256: sha256File(productPaths().exe),
    dirty_tree: source.dirty,
    ...rest,
  };
  const receipt = writeReceipt(outDir, `real-${clientId}.json`, body, activeSecrets);
  const summary = Object.fromEntries(Object.entries(rest.claims).map(([k, v]) => [k, v.result]));
  console.log(formatSafeJson({ receipt: receipt.file, sha256: receipt.sha256, observed_client: rest.observed_client, summary }, activeSecrets));
  for (const claim of Object.values(rest.claims)) {
    for (const check of claim.checks.filter((c) => !c.ok)) console.log(formatSafeLine(`FAIL ${claim.id} ${check.name} :: ${check.detail}`, activeSecrets));
  }
  process.exitCode = Object.values(summary).every((r) => r === 'PASS') ? 0 : 1;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [phase] = process.argv.slice(2);
  if (phase === 'synthetic') await syntheticPhase();
  else if (phase === 'real') await realPhase(process.argv[3]);
  else if (phase === 'matrix') await matrixPhase();
  else if (phase === 'serve') await servePhase(process.argv.includes('--proxy'));
  else {
    console.error('Usage: run.mjs synthetic | real <client> | matrix | serve [--proxy]');
    process.exitCode = 2;
  }
}

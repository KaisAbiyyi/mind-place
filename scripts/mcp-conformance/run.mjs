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
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { findSecretsOnDisk, scrubText } from '../lib/e2e-secret-guard.mjs';
import { launchApp, portIsBusy } from './app-driver.mjs';
import { seedCorpus } from './corpus.mjs';
import { runSyntheticSuite } from './suite.mjs';
import { createWire } from './wire.mjs';
import { runRealClient } from './real.mjs';
import { CLAIMS, CLIENTS, renderMatrixMarkdown, validateMatrix } from './matrix.mjs';

export const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const git = (...args) => spawnSync('git', args, { cwd: repoRoot, encoding: 'utf8' }).stdout.trim();

export function sourceIdentity() {
  const sha = git('rev-parse', 'HEAD');
  const dirty = git('status', '--porcelain', '--untracked-files=no').length > 0;
  return {
    sha,
    dirty,
    // The binary is built from Rust sources and the frontend. Binding both lets a reader see exactly what the executable embodies.
    trees: { crates: git('rev-parse', 'HEAD:crates'), src_tauri: git('rev-parse', 'HEAD:src-tauri'), cargo_lock: git('rev-parse', 'HEAD:Cargo.lock'), src: git('rev-parse', 'HEAD:src') },
  };
}

export function productPaths() {
  const dir = path.join(repoRoot, 'release', `${process.platform}-${process.arch}`);
  return { exe: path.join(dir, 'mind-place.exe'), host: path.join(dir, 'mind-place-capture-host.exe') };
}

export function sha256File(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
}

/** Fresh disposable runtime: archive seeded through the real capture host, app launched, unlocked, sharing on. */
export async function startSession({ label, corpusOptions, log = () => {} }) {
  const { exe, host } = productPaths();
  if (!fs.existsSync(exe) || !fs.existsSync(host)) throw new Error('Missing packaged app: run npm run build first');
  if (await portIsBusy(43731)) throw new Error('MCP port 43731 is in use: close any running Mind Place first');
  const base = process.env.MP_CONFORMANCE_DIR ?? 'D:/mp-conf';
  fs.mkdirSync(base, { recursive: true });
  const runtime = fs.mkdtempSync(path.join(base, `${label}-`));
  const dataDir = path.join(runtime, 'archive-v1');
  const corpus = seedCorpus({ host, archiveRoot: dataDir, ...corpusOptions });
  log(`seeded ${Object.keys(corpus).length} conversations`);
  const app = await launchApp({ exe, runtimeDir: runtime, dataDir });
  const secrets = [];
  const unlockSecret = await app.firstRunUnlock();
  secrets.push(unlockSecret);
  await app.startSharing();
  log('app unlocked, sharing on');
  return { app, corpus, unlockSecret, secrets, runtime, exe };
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
  const log = (m) => console.error(`[synthetic] ${m}`);
  const session = await startSession({ label: 'syn', log });
  const { app, corpus, unlockSecret, secrets } = session;
  let result;
  let failure = null;
  try {
    result = await runSyntheticSuite({ wire: createWire(), app, corpus, unlockSecret, secrets, log });
  } catch (error) {
    failure = String(error?.message ?? error);
  } finally {
    await app.close();
  }
  const body = {
    kind: 'SYNTHETIC_HTTP reference run against the packaged app (disposable synthetic corpus). Not proof of any provider client.',
    source,
    executable_sha256: sha256File(session.exe),
    dirty_tree: source.dirty,
    page_errors: app.pageErrors,
    corpus_summary: Object.fromEntries(Object.entries(corpus).map(([k, v]) => [k, v.revisions.map((r) => ({ messages: r.message_count, raw_sha256: r.raw_sha256 }))])),
    schema_features: result?.schemaFeatures ?? [],
    harness_failure: result?.harnessFailure ?? null,
    failure,
    claims: result?.claims ?? null,
  };
  const receipt = writeReceipt(outDir, 'synthetic.json', body, secrets);
  const summary = Object.fromEntries(Object.entries(result?.claims ?? {}).map(([k, v]) => [k, v.result]));
  console.log(JSON.stringify({ receipt: receipt.file, sha256: receipt.sha256, summary, failure }, null, 2));
  for (const claim of Object.values(result?.claims ?? {})) {
    for (const check of claim.checks.filter((c) => !c.ok)) console.log(`FAIL ${claim.id} ${check.name} :: ${check.detail}`);
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
  const matrix = { sha: source.sha, product_tree_id: treeId(source.trees), generated_at: new Date().toISOString(), executable_sha256: JSON.parse(fs.readFileSync(path.join(outDir, 'synthetic.json'), 'utf8')).executable_sha256, rust_trees: source.trees, cells };
  problems.push(...validateMatrix(matrix));
  fs.writeFileSync(path.join(docsDir, 'matrix.json'), JSON.stringify(matrix, null, 2) + '\n');
  fs.writeFileSync(path.join(docsDir, 'PARITY-MATRIX.md'), renderMatrixMarkdown(matrix));
  const leaks = findSecretsOnDisk(docsDir, []);
  problems.push(...leaks);
  console.log(JSON.stringify({ sha: source.sha, problems }, null, 2));
  process.exitCode = problems.length ? 1 : 0;
}

/**
 * Owner test card helper: a disposable, unlocked Mind Place with the synthetic conformance corpus, left open
 * for the owner to connect a client that cannot be driven from here (hosted ChatGPT, Gemini app, Claude app).
 * The owner creates the access key and picks chats in the real Connections screen. Nothing private is loaded.
 */
async function servePhase() {
  const log = (m) => console.error(`[serve] ${m}`);
  const session = await startSession({ label: 'serve', corpusOptions: { longMessages: 6, longBytes: 500, mediumMessages: 10, mediumBytes: 1400 }, log });
  console.log('Disposable Mind Place is running with a synthetic corpus (4 chats). It is unlocked and sharing is on.');
  console.log('In the app: open Connections, create access for your client, then Choose chats and allow ONLY "Conformance medium conversation".');
  console.log('Follow docs/qualification/mcp-parity/OWNER-TEST-CARDS.md. Press Ctrl-C here when done (this closes the app and discards the archive).');
  const stop = async () => { await session.app.close(); process.exit(0); };
  process.on('SIGINT', stop);
  await new Promise(() => {});
}

async function realPhase(clientId) {
  const source = sourceIdentity();
  const outDir = path.join(repoRoot, 'outputs', 'mcp-conformance', source.sha.slice(0, 12));
  const log = (m) => console.error(`[real:${clientId}] ${m}`);
  const result = await runRealClient(clientId, { startSession, log });
  const { secrets = [], ...rest } = result;
  if (rest.blocked) {
    const blockedReceipt = writeReceipt(outDir, `blocked-${clientId}.json`, { source, clientId, installed: rest.installed, blocked: rest.blocked }, secrets);
    console.log(JSON.stringify({ clientId, blocked: rest.blocked, receipt: blockedReceipt.file }, null, 2));
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
  const receipt = writeReceipt(outDir, `real-${clientId}.json`, body, secrets);
  const summary = Object.fromEntries(Object.entries(rest.claims).map(([k, v]) => [k, v.result]));
  console.log(JSON.stringify({ receipt: receipt.file, sha256: receipt.sha256, observed_client: rest.observed_client, summary }, null, 2));
  for (const claim of Object.values(rest.claims)) for (const check of claim.checks.filter((c) => !c.ok)) console.log(`FAIL ${claim.id} ${check.name} :: ${check.detail}`);
  process.exitCode = Object.values(summary).every((r) => r === 'PASS') ? 0 : 1;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [phase] = process.argv.slice(2);
  if (phase === 'synthetic') await syntheticPhase();
  else if (phase === 'real') await realPhase(process.argv[3]);
  else if (phase === 'matrix') await matrixPhase();
  else if (phase === 'serve') await servePhase();
  else {
    console.error('Usage: run.mjs synthetic | real <client> | matrix');
    process.exitCode = 2;
  }
}

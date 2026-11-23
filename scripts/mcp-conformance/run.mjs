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

async function realPhase(clientId) {
  const source = sourceIdentity();
  const outDir = path.join(repoRoot, 'outputs', 'mcp-conformance', source.sha.slice(0, 12));
  const log = (m) => console.error(`[real:${clientId}] ${m}`);
  const result = await runRealClient(clientId, { startSession, log });
  const { secrets = [], ...rest } = result;
  if (rest.blocked) {
    console.log(JSON.stringify({ clientId, blocked: rest.blocked }, null, 2));
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
  else {
    console.error('Usage: run.mjs synthetic | real <client> | matrix');
    process.exitCode = 2;
  }
}

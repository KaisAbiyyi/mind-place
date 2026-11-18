import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { evaluate, loadSuite, oraclePredictions, readSmallJson, validateSuite } from '../scripts/benchmark-continuity.mjs';

const clone = (value) => structuredClone(value);
const suite = loadSuite();

test('MP-10A frozen synthetic data covers cross-provider, projects, time, source scopes and abstention', () => {
  assert.equal(validateSuite(suite), true);
  assert.equal(suite.corpus.sources.length, 24);
  assert.equal(suite.gold.cases.length, 18);
  assert.deepEqual([...new Set(suite.corpus.sources.map(s => s.provider))].sort(), ['chatgpt', 'claude', 'gemini', 'generic']);
  assert.deepEqual([...new Set(suite.corpus.sources.map(s => s.project))].sort(), ['aster', 'beryl']);
  assert.ok(suite.corpus.sources.some(s => s.scope === 'restricted'));
  assert.ok(suite.gold.cases.some(q => q.abstain));
  assert.ok(suite.gold.cases.some(q => q.prohibited_claims.length > 0));
  assert.match(suite.fixture_sha256.corpus, /^[a-f0-9]{64}$/);
  assert.match(suite.fixture_sha256.gold, /^[a-f0-9]{64}$/);
});

test('oracle proves case-to-evidence and scoring contracts, NOT product retrieval quality', () => {
  const oracle = oraclePredictions(suite);
  const report = evaluate(suite, oracle);
  assert.equal(report.summary.answer_accuracy, 1);
  assert.equal(report.summary.macro_recall_at_k, 1);
  assert.equal(report.summary.mean_reciprocal_rank, 1);
  assert.equal(report.summary.macro_ndcg_at_k, 1);
  assert.equal(report.summary.privacy_violation_count, 0);
  assert.equal(report.summary.unsupported_claim_count, 0);
  assert.equal(report.summary.abstention_accuracy, 1);
  assert.equal(report.summary.abstention_cases, 2);
  assert.deepEqual(report.cases.map(c => c.id), suite.gold.cases.map(c => c.id));
});

test('ranking metrics penalize misplaced and missing relevant evidence', () => {
  const predictions = oraclePredictions(suite);
  const entry = predictions.cases.find(c => c.id === 'q01');
  entry.hits = ['e10', 'e03'];
  const report = evaluate(suite, predictions);
  const q = report.cases.find(c => c.id === 'q01');
  assert.equal(q.recall_at_k, 0.5);
  assert.equal(q.precision_at_k, 0.2);
  assert.equal(q.reciprocal_rank, 0.5);
  assert.ok(q.ndcg_at_k > 0 && q.ndcg_at_k < 1);
  assert.equal(q.answer_correct, true); // e03 supports the required final decision
});

test('privacy and restricted evidence fail even when an answer abstains', () => {
  const predictions = oraclePredictions(suite);
  predictions.cases.find(c => c.id === 'q14').hits = ['e20'];
  const report = evaluate(suite, predictions);
  assert.equal(report.summary.privacy_violation_count, 1);
  assert.equal(report.cases.find(c => c.id === 'q14').answer_correct, false);
  assert.deepEqual(report.cases.find(c => c.id === 'q14').forbidden_hits, ['e20']);
});

test('hallucinated, stale or wrong-project claims are flagged separately from retrieval', () => {
  const predictions = oraclePredictions(suite);
  predictions.cases.find(c => c.id === 'q01').claims = ['old_vector_proposal'];
  predictions.cases.find(c => c.id === 'q13').claims = ['beryl_memory_budget'];
  predictions.cases.find(c => c.id === 'q13').abstained = false;
  const report = evaluate(suite, predictions);
  assert.equal(report.cases.find(c => c.id === 'q01').answer_correct, false);
  assert.ok(report.cases.find(c => c.id === 'q01').prohibited_claims.includes('old_vector_proposal'));
  assert.equal(report.cases.find(c => c.id === 'q13').answer_correct, false);
  assert.ok(report.summary.unsupported_claim_count >= 2);
  assert.ok(report.summary.prohibited_claim_count >= 2);
});

test('unknown, duplicate, incomplete or non-finite prediction input fails closed', () => {
  const oracle = oraclePredictions(suite);
  const incorrect = [
    () => { const p = clone(oracle); p.dataset = 'another-dataset'; return p; },
    () => { const p = clone(oracle); p.cases.pop(); return p; },
    () => { const p = clone(oracle); p.cases[0].id = 'q02'; return p; },
    () => { const p = clone(oracle); p.cases[0].hits = ['nonexistent']; return p; },
    () => { const p = clone(oracle); p.cases[0].hits = ['e03', 'e03']; return p; },
    () => { const p = clone(oracle); p.cases[0].latency_ms = -1; return p; },
    () => { const p = clone(oracle); p.cases[0].context_estimated_tokens = 1.4; return p; },
  ];
  for (const make of incorrect) assert.throws(() => evaluate(suite, make()), /Invalid continuity benchmark/);
});

test('gold suite rejects unproven required claims and relevance to restricted sources', () => {
  const badClaim = clone(suite);
  badClaim.gold.cases[0].required_claims.push('nonexistent_claim');
  assert.throws(() => validateSuite(badClaim), /unproven required claim/);

  const badEvidence = clone(suite);
  badEvidence.gold.cases[0].relevant.e20 = 3;
  assert.throws(() => validateSuite(badEvidence), /relevant evidence/);

  const badAbstain = clone(suite);
  badAbstain.gold.cases.find(c => c.id === 'q14').abstain = false;
  assert.throws(() => validateSuite(badAbstain), /abstention contract/);
});

test('scores are deterministic under prediction input case reorder', () => {
  const p = oraclePredictions(suite);
  const original = evaluate(suite, p);
  p.cases.reverse();
  assert.deepEqual(evaluate(suite, p), original);
});

const withTempDir = (fn) => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mp-continuity-'));
  try {
    return fn(dir);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
};

const cliScriptPath = fileURLToPath(new URL('../scripts/benchmark-continuity.mjs', import.meta.url));
const MAX_PREDICTION_BYTES = 2 * 1024 * 1024;

test('readSmallJson enforces exact 2 MiB byte ceiling via helper and CLI --predictions', () => {
  withTempDir((dir) => {
    const baseJson = JSON.stringify(oraclePredictions(suite));
    const padExact = MAX_PREDICTION_BYTES - Buffer.byteLength(baseJson, 'utf8');
    assert.ok(padExact > 0);
    const exactFile = path.join(dir, 'exact-2mib.json');
    fs.writeFileSync(exactFile, baseJson + ' '.repeat(padExact), 'utf8');
    assert.equal(fs.statSync(exactFile).size, MAX_PREDICTION_BYTES);

    const parsed = readSmallJson(exactFile);
    assert.equal(parsed.cases.length, suite.gold.cases.length);

    const okCli = spawnSync(process.execPath, [cliScriptPath, '--predictions', exactFile], {
      encoding: 'utf8',
      windowsHide: true,
    });
    assert.equal(okCli.status, 0, okCli.stderr);
    const cliReport = JSON.parse(okCli.stdout);
    assert.equal(cliReport.mode, 'submitted_predictions');
    assert.equal(cliReport.summary.answer_accuracy, 1);

    const overFile = path.join(dir, 'over-2mib.json');
    fs.writeFileSync(overFile, baseJson + ' '.repeat(padExact + 1), 'utf8');
    assert.throws(() => readSmallJson(overFile), /Invalid continuity benchmark: prediction input exceeds 2 MiB/);

    const failCli = spawnSync(process.execPath, [cliScriptPath, '--predictions', overFile], {
      encoding: 'utf8',
      windowsHide: true,
    });
    assert.equal(failCli.status, 1);
    assert.match(failCli.stderr, /prediction input exceeds 2 MiB/);
  });
});

test('readSmallJson rejects directories, non-regular descriptors, and invalid paths without leaking fds', () => {
  withTempDir((dir) => {
    assert.throws(() => readSmallJson(''), /prediction input path must be a non-empty string/);
    assert.throws(() => readSmallJson('bad\0path.json'), /prediction input path must be a non-empty string/);
    assert.throws(() => readSmallJson(dir), /prediction input must be a regular file/);

    const filePath = path.join(dir, 'sample.json');
    fs.writeFileSync(filePath, '{"ok":true}', 'utf8');

    const openFds = new Set();
    let readCalled = false;
    const nonRegularFs = {
      constants: fs.constants,
      openSync: (target, flags) => {
        const fd = fs.openSync(target, flags);
        openFds.add(fd);
        return fd;
      },
      fstatSync: () => ({
        isFile: () => false,
        size: 11n,
        dev: 1n,
        ino: 1n,
      }),
      lstatSync: fs.lstatSync,
      readSync: (...args) => {
        readCalled = true;
        return fs.readSync(...args);
      },
      closeSync: (fd) => {
        openFds.delete(fd);
        return fs.closeSync(fd);
      },
    };
    assert.throws(() => readSmallJson(filePath, { fs: nonRegularFs }), /prediction input must be a regular file/);
    assert.equal(readCalled, false);
    assert.equal(openFds.size, 0);
  });
});

test('readSmallJson rejects symlinks via O_NOFOLLOW or post-open leaf verification without fd leaks', () => {
  withTempDir((dir) => {
    const targetFile = path.join(dir, 'target.json');
    fs.writeFileSync(targetFile, '{"ok":true}', 'utf8');
    const linkFile = path.join(dir, 'link.json');

    let symlinkCreated = false;
    try {
      fs.symlinkSync(targetFile, linkFile);
      symlinkCreated = true;
    } catch (error) {
      if (!error || (error.code !== 'EPERM' && error.code !== 'ENOTSUP')) throw error;
    }
    if (symlinkCreated) {
      assert.throws(() => readSmallJson(linkFile), /prediction input must not be a symbolic link/);
    }

    let observedFlags = 0;
    const posixOloopFs = {
      constants: { O_RDONLY: 0, O_NOFOLLOW: 0x20000, O_NONBLOCK: 0x800 },
      openSync: (_p, flags) => {
        observedFlags = flags;
        const err = new Error('ELOOP: too many levels of symbolic links');
        err.code = 'ELOOP';
        throw err;
      },
      fstatSync: fs.fstatSync,
      lstatSync: fs.lstatSync,
      readSync: fs.readSync,
      closeSync: fs.closeSync,
    };
    assert.throws(() => readSmallJson(targetFile, { fs: posixOloopFs }), /prediction input must not be a symbolic link/);
    assert.ok((observedFlags & 0x20000) !== 0, 'O_NOFOLLOW flag must be passed when available');

    const openFds = new Set();
    let readCalled = false;
    const windowsSymlinkFs = {
      constants: { O_RDONLY: 0 },
      openSync: (target, flags) => {
        const fd = fs.openSync(target, flags);
        openFds.add(fd);
        return fd;
      },
      fstatSync: fs.fstatSync,
      lstatSync: () => ({
        isSymbolicLink: () => true,
        isFile: () => false,
        dev: 1n,
        ino: 1n,
      }),
      readSync: (...args) => {
        readCalled = true;
        return fs.readSync(...args);
      },
      closeSync: (fd) => {
        openFds.delete(fd);
        return fs.closeSync(fd);
      },
    };
    assert.throws(() => readSmallJson(targetFile, { fs: windowsSymlinkFs }), /prediction input must not be a symbolic link/);
    assert.equal(readCalled, false);
    assert.equal(openFds.size, 0);
  });
});

test('readSmallJson enforces 2 MiB ceiling when a file grows concurrently after open/fstatSync', () => {
  withTempDir((dir) => {
    const growFile = path.join(dir, 'growing.json');
    fs.writeFileSync(growFile, '{"padding":"', 'utf8');

    const openFds = new Set();
    let appended = false;
    let totalReadCalls = 0;
    const growingFs = {
      constants: fs.constants,
      openSync: (target, flags) => {
        const fd = fs.openSync(target, flags);
        openFds.add(fd);
        return fd;
      },
      fstatSync: fs.fstatSync,
      lstatSync: fs.lstatSync,
      readSync: (fd, buf, offset, length, position) => {
        totalReadCalls += 1;
        if (!appended) {
          appended = true;
          // Grow real file past 2 MiB AFTER openSync + fstatSync already saw 12 bytes
          fs.appendFileSync(growFile, 'a'.repeat(MAX_PREDICTION_BYTES) + '"}', 'utf8');
        }
        return fs.readSync(fd, buf, offset, length, position);
      },
      closeSync: (fd) => {
        openFds.delete(fd);
        return fs.closeSync(fd);
      },
    };

    assert.throws(() => readSmallJson(growFile, { fs: growingFs }), /Invalid continuity benchmark: prediction input exceeds 2 MiB/);
    assert.ok(appended);
    assert.ok(totalReadCalls >= 1);
    assert.equal(openFds.size, 0);
  });
});

test('readSmallJson binds reads to opened fd on swap and rejects Windows post-open path identity changes', () => {
  withTempDir((dir) => {
    const primaryFile = path.join(dir, 'primary.json');
    const swappedFile = path.join(dir, 'swapped.json');
    fs.writeFileSync(primaryFile, JSON.stringify({ ok: true, source: 'original-fd' }), 'utf8');
    fs.writeFileSync(swappedFile, '{invalid-json-from-swapped-path', 'utf8');

    // Scenario A: With O_NOFOLLOW support, subsequent path replacement cannot alter bytes read from open fd.
    const openFds = new Set();
    let swappedAfterOpen = false;
    const descriptorPinnedFs = {
      constants: { O_RDONLY: fs.constants.O_RDONLY ?? 0, O_NOFOLLOW: fs.constants.O_NOFOLLOW || 0x20000 },
      openSync: (target) => {
        const fd = fs.openSync(target, fs.constants.O_RDONLY ?? 0);
        openFds.add(fd);
        if (!swappedAfterOpen) {
          swappedAfterOpen = true;
          try {
            fs.renameSync(swappedFile, primaryFile);
          } catch (error) {
            // Windows denies rename over open handle with EPERM; path identity remains pinned to open fd regardless
            if (!error || error.code !== 'EPERM') throw error;
          }
        }
        return fd;
      },
      fstatSync: fs.fstatSync,
      lstatSync: fs.lstatSync,
      readSync: fs.readSync,
      closeSync: (fd) => {
        openFds.delete(fd);
        return fs.closeSync(fd);
      },
    };
    const pinnedData = readSmallJson(primaryFile, { fs: descriptorPinnedFs });
    assert.deepEqual(pinnedData, { ok: true, source: 'original-fd' });
    assert.equal(openFds.size, 0);

    // Scenario B: Without O_NOFOLLOW (Windows fallback), changed (dev, ino) at path fails closed before reading.
    fs.writeFileSync(primaryFile, JSON.stringify({ ok: true, source: 'primary' }), 'utf8');
    fs.writeFileSync(swappedFile, '{poison', 'utf8');
    let readCalled = false;
    const identityMismatchFs = {
      constants: { O_RDONLY: 0 },
      openSync: (target, flags) => {
        const fd = fs.openSync(target, flags);
        openFds.add(fd);
        return fd;
      },
      fstatSync: fs.fstatSync,
      lstatSync: () => fs.lstatSync(swappedFile, { bigint: true }),
      readSync: (...args) => {
        readCalled = true;
        return fs.readSync(...args);
      },
      closeSync: (fd) => {
        openFds.delete(fd);
        return fs.closeSync(fd);
      },
    };
    assert.throws(() => readSmallJson(primaryFile, { fs: identityMismatchFs }), /prediction input path changed during open/);
    assert.equal(readCalled, false);
    assert.equal(openFds.size, 0);
  });
});

import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import {
  assertPinnedCrateNotices,
  licenseFileTexts,
  lockedVersions,
  noticeSections,
  pinnedThirdPartyCrates,
} from '../scripts/notice-coverage.mjs';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const SEP = '='.repeat(72);
const MIT_TEXT = 'Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction.';

function section(name, version, license, files = { LICENSE: MIT_TEXT }) {
  const body = Object.entries(files).map(([file, text]) => `--- ${file} ---\n${text}`).join('\n');
  return `\n${SEP}\n${name} ${version}\nDeclared license: ${license}\n\n${body || `License declaration: ${license}; package source: ${name}`}`;
}

function fixtureRoot(manifests, lock) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mp-notice-cov-'));
  fs.mkdirSync(path.join(root, 'src-tauri'));
  fs.writeFileSync(path.join(root, 'src-tauri', 'Cargo.toml'), '[package]\nname = "app"\n\n[dependencies]\n');
  for (const [crate, text] of Object.entries(manifests)) {
    fs.mkdirSync(path.join(root, 'crates', crate), { recursive: true });
    fs.writeFileSync(path.join(root, 'crates', crate, 'Cargo.toml'), text);
  }
  fs.writeFileSync(path.join(root, 'Cargo.lock'), lock);
  return root;
}

const LOCK = `version = 4

[[package]]
name = "parsegum"
version = "1.2.3"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "00"

[[package]]
name = "localcrate"
version = "0.1.0"
`;
const MANIFEST = `[package]
name = "core"

[dependencies]
parsegum = { version = "=1.2.3", default-features = false }
localcrate = { path = "../localcrate", version = "=0.1.0" }
loose = "1"

[dev-dependencies]
testonly = "=9.9.9"
`;

test('only exact-pinned shipped registry crates are required; path, loose and dev dependencies are not', () => {
  const root = fixtureRoot({ core: MANIFEST }, LOCK);
  try {
    assert.deepEqual([...pinnedThirdPartyCrates(root)], [['parsegum', '1.2.3']]);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

test('a pinned crate with its own section, declared license and license text passes', () => {
  const root = fixtureRoot({ core: MANIFEST }, LOCK);
  try {
    const notices = `Mind Place — third-party source and license notices${section('parsegum', '1.2.3', 'MIT')}`;
    assert.deepEqual(assertPinnedCrateNotices(notices, root), ['parsegum']);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

test('missing section, wrong version, missing license text and fallback declaration all fail', () => {
  const root = fixtureRoot({ core: MANIFEST }, LOCK);
  try {
    const head = 'Mind Place — third-party source and license notices';
    assert.throws(() => assertPinnedCrateNotices(`${head}${section('other', '1.0.0', 'MIT')}`, root), /parsegum 1\.2\.3: no third-party notice section/);
    assert.throws(() => assertPinnedCrateNotices(`${head}${section('parsegum', '1.2.2', 'MIT')}`, root), /parsegum 1\.2\.3: no third-party notice section/);
    assert.throws(() => assertPinnedCrateNotices(`${head}${section('parsegum', '1.2.3', 'MIT', {})}`, root), /no license text included/);
    assert.throws(() => assertPinnedCrateNotices(`${head}${section('parsegum', '1.2.3', 'MIT', { README: MIT_TEXT })}`, root), /no license text included/);
    assert.throws(() => assertPinnedCrateNotices(`${head}${section('parsegum', '1.2.3', 'See source license')}`, root), /no declared license/);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

test('a manifest pin that drifted from Cargo.lock is reported', () => {
  const root = fixtureRoot({ core: MANIFEST.replace('=1.2.3', '=1.2.4') }, LOCK);
  try {
    assert.throws(() => assertPinnedCrateNotices(`x${section('parsegum', '1.2.4', 'MIT')}`, root), /manifest pins 1\.2\.4 but Cargo\.lock has 1\.2\.3/);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

test('notice parsing keeps each license file separate', () => {
  const text = `head${section('a', '1.0.0', 'MIT OR Apache-2.0', { 'LICENSE-MIT': 'mit text', 'LICENSE-APACHE': 'apache text' })}${section('b', '2.0.0', 'MIT', {})}`;
  const sections = noticeSections(text);
  assert.equal(sections.get('a 1.0.0').license, 'MIT OR Apache-2.0');
  assert.deepEqual(licenseFileTexts(sections.get('a 1.0.0').body), ['mit text', 'apache text']);
  assert.deepEqual(licenseFileTexts(sections.get('b 2.0.0').body), []);
  assert.deepEqual(lockedVersions(LOCK).get('parsegum'), ['1.2.3']);
  assert.equal(lockedVersions(LOCK).has('localcrate'), false);
});

test('the repository pins html5gum for shipping and Cargo.lock agrees, so its notice is mandatory', () => {
  const pinned = pinnedThirdPartyCrates(repo);
  assert.equal(pinned.get('html5gum'), '0.8.0');
  assert.deepEqual(lockedVersions(fs.readFileSync(path.join(repo, 'Cargo.lock'), 'utf8')).get('html5gum'), ['0.8.0']);
});

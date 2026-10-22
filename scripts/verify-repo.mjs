import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const readJson = (relativePath) => JSON.parse(fs.readFileSync(path.join(root, relativePath), 'utf8'));

const product = readJson('package.json');
const tauri = readJson('src-tauri/tauri.conf.json');
const extension = readJson('extension/manifest.json');
const cargo = fs.readFileSync(path.join(root, 'Cargo.toml'), 'utf8');
const cargoVersion = cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
const cargoLicense = cargo.match(/^license\s*=\s*"([^"]+)"/m)?.[1];

if (!cargoVersion) throw new Error('Cannot read workspace version from Cargo.toml');
if (product.version !== cargoVersion || product.version !== tauri.version) {
  throw new Error(`Version mismatch: package=${product.version}, cargo=${cargoVersion}, tauri=${tauri.version}`);
}
if (product.license !== 'MIT' || cargoLicense !== product.license || readJson('package-lock.json').packages[''].license !== 'MIT') {
  throw new Error(`License mismatch: package=${product.license}, cargo=${cargoLicense}, npm-lock=${readJson('package-lock.json').packages[''].license}`);
}
const licenseText = fs.readFileSync(path.join(root, 'LICENSE'), 'utf8');
if (!licenseText.startsWith('MIT License\n\nCopyright (c) 2026 Kais Abiyyi\n') ||
    !licenseText.includes('Permission is hereby granted, free of charge') ||
    !licenseText.includes('THE SOFTWARE IS PROVIDED "AS IS"')) {
  throw new Error('Canonical MIT LICENSE or first-party copyright notice missing');
}
const activeDeclarations = ['README.md', 'CONTRIBUTING.md', 'package.json', 'package-lock.json', 'Cargo.toml'];
for (const file of activeDeclarations) {
  if (/AGPL-3\.0-or-later|GNU Affero General Public License v3/i.test(fs.readFileSync(path.join(root, file), 'utf8'))) {
    throw new Error(`Stale first-party AGPL declaration in ${file}`);
  }
}

if (extension.manifest_version !== 3) throw new Error('Capture extension must use Manifest V3');
if (JSON.stringify(extension.permissions) !== JSON.stringify(['activeTab', 'scripting', 'nativeMessaging', 'storage'])) {
  throw new Error('Unexpected capture extension permissions');
}
if (JSON.stringify(extension.optional_host_permissions) !== JSON.stringify(['https://chatgpt.com/*'])) {
  throw new Error('Only optional ChatGPT origin access may be declared');
}
if (extension.content_scripts?.length || extension.host_permissions?.length) {
  throw new Error('Capture extension cannot contain unconditional page scripts or host permissions');
}

const tracked = spawnSync('git', ['ls-files'], { cwd: root, encoding: 'utf8', windowsHide: true });
if (tracked.status !== 0) throw new Error(tracked.stderr || 'git ls-files failed');
const trackedFiles = tracked.stdout.split(/\r?\n/).filter(Boolean);
const forbidden = trackedFiles.filter((file) =>
  file === '.gitmodules' ||
  file === 'cos' ||
  file.startsWith('cos/') ||
  file.startsWith('outputs/') ||
  file.startsWith('release/') ||
  file.startsWith('target/') ||
  file.startsWith('node_modules/') ||
  file === 'THIRD-PARTY-NOTICES.txt'
);
if (forbidden.length) throw new Error(`Repository contains generated or legacy tracked paths: ${forbidden.join(', ')}`);

console.log(`Repository contract OK — Mind Place ${product.version}, extension ${extension.version}`);

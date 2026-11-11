// Dependency-specific third-party notice coverage.
//
// A generic "a notice file exists" check passes even when a newly shipped crate has no
// notice. The distribution license check therefore derives the exact-pinned third-party
// Rust crates from the Cargo manifests and the locked version from Cargo.lock, and requires
// each one to have its own section, declared license and license text in the notices that
// ship with the product.
import fs from 'node:fs';
import path from 'node:path';

const SEPARATOR = '='.repeat(72);
const SHIPPED_SECTIONS = new Set(['dependencies', 'build-dependencies']);

function manifestPaths(root) {
  const paths = [path.join(root, 'src-tauri', 'Cargo.toml')];
  const crates = path.join(root, 'crates');
  if (fs.existsSync(crates)) {
    for (const entry of fs.readdirSync(crates, { withFileTypes: true })) {
      const manifest = path.join(crates, entry.name, 'Cargo.toml');
      if (entry.isDirectory() && fs.existsSync(manifest)) paths.push(manifest);
    }
  }
  return paths.filter(file => fs.existsSync(file));
}

/** Exact-pinned (`=x.y.z`) registry dependencies of the product, as Map<name, version>. */
export function pinnedThirdPartyCrates(root) {
  const pinned = new Map();
  for (const manifest of manifestPaths(root)) {
    let section = '';
    for (const raw of fs.readFileSync(manifest, 'utf8').split(/\r?\n/)) {
      const line = raw.trim();
      const header = /^\[(.+)\]$/.exec(line);
      if (header) { section = header[1].replace(/^target\..+\./, ''); continue; }
      if (!SHIPPED_SECTIONS.has(section) || line.startsWith('#')) continue;
      const entry = /^([A-Za-z0-9_-]+)\s*=\s*(.+)$/.exec(line);
      if (!entry) continue;
      const [, name, value] = entry;
      if (/\bpath\s*=/.test(value)) continue;
      const version = /^"=([^"]+)"/.exec(value) ?? /\bversion\s*=\s*"=([^"]+)"/.exec(value);
      if (!version) continue;
      const previous = pinned.get(name);
      if (previous && previous !== version[1]) {
        throw new Error(`Pinned crate ${name} has conflicting versions: ${previous} and ${version[1]}`);
      }
      pinned.set(name, version[1]);
    }
  }
  return pinned;
}

/** Locked registry package versions, as Map<name, string[]>. */
export function lockedVersions(lockText) {
  const locked = new Map();
  for (const block of lockText.split(/\r?\n\r?\n/)) {
    const name = /^name = "([^"]+)"$/m.exec(block)?.[1];
    const version = /^version = "([^"]+)"$/m.exec(block)?.[1];
    if (!name || !version || !/^source = "registry\+/m.test(block)) continue;
    locked.set(name, [...(locked.get(name) ?? []), version]);
  }
  return locked;
}

/** Notice sections keyed by `name version`, as Map<string, { license, body }>. */
export function noticeSections(noticesText) {
  const sections = new Map();
  for (const chunk of noticesText.split(`\n${SEPARATOR}\n`).slice(1)) {
    const heading = /^(\S+) (\S+)\nDeclared license: ([^\n]*)\n/.exec(chunk);
    if (heading) sections.set(`${heading[1]} ${heading[2]}`, { license: heading[3].trim(), body: chunk });
  }
  return sections;
}

/** Text of each bundled license/notice file (`--- <file> ---` blocks) in one notice section. */
export function licenseFileTexts(body) {
  const texts = [];
  let current = null;
  for (const line of body.split('\n')) {
    const header = /^--- (.+) ---$/.exec(line);
    if (header) {
      current = /(?:^|\/)(?:licen[cs]e|copying|notice)/i.test(header[1]) ? [] : null;
      if (current) texts.push(current);
    } else if (current) {
      current.push(line);
    }
  }
  return texts.map(lines => lines.join('\n'));
}

/**
 * Throws unless every exact-pinned shipped crate has a notice section for its locked
 * version, a declared license, and at least one real license file (not the generator's
 * "no license file found" fallback).
 */
export function assertPinnedCrateNotices(noticesText, root) {
  const pinned = pinnedThirdPartyCrates(root);
  if (!pinned.size) throw new Error('No exact-pinned third-party crates were found; notice coverage cannot be verified');
  const locked = lockedVersions(fs.readFileSync(path.join(root, 'Cargo.lock'), 'utf8'));
  const sections = noticeSections(noticesText);
  const problems = [];
  for (const [name, version] of [...pinned].sort(([a], [b]) => a.localeCompare(b))) {
    if (!(locked.get(name) ?? []).includes(version)) {
      problems.push(`${name}: manifest pins ${version} but Cargo.lock has ${(locked.get(name) ?? ['nothing']).join(', ')}`);
      continue;
    }
    const section = sections.get(`${name} ${version}`);
    if (!section) { problems.push(`${name} ${version}: no third-party notice section`); continue; }
    if (!section.license || section.license === 'See source license') {
      problems.push(`${name} ${version}: no declared license`);
    }
    const licenseText = licenseFileTexts(section.body).join('\n');
    if (licenseText.replace(/\s+/g, ' ').trim().length < 100) {
      problems.push(`${name} ${version}: no license text included`);
    }
  }
  if (problems.length) throw new Error(`Third-party notice coverage failed:\n- ${problems.join('\n- ')}`);
  return [...pinned.keys()];
}

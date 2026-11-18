// Dependency-specific third-party notice coverage.
//
// A generic "a notice file exists" check passes even when a newly shipped crate has no
// notice. The distribution license check therefore derives the exact-pinned third-party
// Rust crates from the Cargo manifests and the locked version from Cargo.lock, and requires
// each one to have its own section, declared license and license text in the notices that
// ship with the product.
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

const SEPARATOR = '='.repeat(72);
const SHIPPED_SECTIONS = new Set(['dependencies', 'build-dependencies']);


const TRANSITIVE_NOTICES = [
  {
    crate: 'selectors',
    version: '0.38.0',
    name: 'LICENSE-MPL-2.0',
    declared: 'MPL-2.0',
    // The selectors crate header points to MPL-2.0. The locked dtoa-short
    // 0.3.5 LICENSE supplies an identical copy of that license text.
    file: 'MPL-2.0.txt',
    sha256: '1f256ecad192880510e84ad60474eab7589218784b9a50bc7ceee34c2b91f1d5',
  },
  {
    crate: 'sqlcipher-src',
    version: '419.0.6+sqlcipher-4.19.0-sqlite-3.53.4-libtomcrypt-1.18.2',
    name: 'sqlcipher/LICENSE-SQLITE-BLESSING',
    declared: 'MIT AND BSD-3-Clause AND blessing AND WTFPL',
    // From the leading notice of sqlcipher/sqlite3.h in the locked crate.
    // Header SHA-256: 8a9d1bff44d75174ca6dea3ea9bac50a6104d86facb566647b8bb839375b7b3a.
    file: 'SQLITE-BLESSING.txt',
    sha256: '79daf4b950cf73d52a38f062a55a9bedc168e7e86b4e935939854ed9c3277d1b',
  },
];

function normalizedNotice(text) {
  return text.replace(/\r\n/g, '\n').trimEnd() + '\n';
}

function noticeDigest(text) {
  return createHash('sha256').update(normalizedNotice(text)).digest('hex');
}

function crateSourceArchive({ crate, version }) {
  return 'https://crates.io/api/v1/crates/' + crate + '/' + encodeURIComponent(version) + '/download';
}

/** Supplemental notices for known registry packages in the resolved graph. */
export function transitiveLicenseNotices(crate, version) {
  return TRANSITIVE_NOTICES
    .filter(entry => entry.crate === crate && entry.version === version)
    .map(entry => {
      const text = normalizedNotice(fs.readFileSync(new URL('./licenses/' + entry.file, import.meta.url), 'utf8'));
      if (noticeDigest(text) !== entry.sha256) throw new Error('Supplemental license text changed: ' + entry.file);
      return { name: entry.name, text, sourceUrl: crateSourceArchive(entry) };
    });
}

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

function noticeSectionEntries(noticesText) {
  const sections = [];
  for (const chunk of noticesText.split(`\n${SEPARATOR}\n`).slice(1)) {
    const heading = /^(\S+) (\S+)\nDeclared license: ([^\n]*)\n/.exec(chunk);
    if (heading) sections.push([`${heading[1]} ${heading[2]}`, { license: heading[3].trim(), body: chunk }]);
  }
  return sections;
}

/** Notice sections keyed by `name version`, as Map<string, { license, body }>. */
export function noticeSections(noticesText) {
  return new Map(noticeSectionEntries(noticesText));
}

function noticeFiles(body) {
  const files = [];
  let current;
  for (const line of body.split('\n')) {
    const header = /^--- (.+) ---$/.exec(line);
    if (header) {
      current = { name: header[1], lines: [] };
      files.push(current);
    } else if (current) {
      current.lines.push(line);
    }
  }
  return files.map(({ name, lines }) => ({ name, text: lines.join('\n') }));
}

/** Text of each bundled license/notice file in one notice section. */
export function licenseFileTexts(body) {
  return noticeFiles(body)
    .filter(file => /(?:^|\/)(?:licen[cs]e|copying|notice)/i.test(file.name))
    .map(file => file.text);
}

/** Require complete, component-specific text for the known locked Windows transitive packages. */
export function assertWindowsTransitiveNotices(noticesText, lockText) {
  const locked = lockedVersions(lockText);
  const sections = noticeSectionEntries(noticesText.replace(/\r\n/g, '\n'));
  const checked = [];
  for (const entry of TRANSITIVE_NOTICES) {
    if (!(locked.get(entry.crate) ?? []).includes(entry.version)) continue;

    const heading = entry.crate + ' ' + entry.version;
    const matches = sections.filter(([key]) => key === heading);
    if (!matches.length) throw new Error(heading + ': no third-party notice section');
    if (matches.length !== 1) throw new Error(heading + ': duplicate third-party notice sections');

    const [, section] = matches[0];
    if (section.license !== entry.declared) throw new Error(heading + ': unexpected declared license');
    const preamble = section.body.split(/^--- /m, 1)[0];
    const origins = preamble.split('\n').filter(line => line.startsWith('Source archive: '));
    if (origins.length !== 1 || origins[0] !== 'Source archive: ' + crateSourceArchive(entry)) {
      throw new Error(heading + ': missing or altered source archive');
    }
    const files = noticeFiles(section.body).filter(file => file.name === entry.name);
    if (files.length !== 1 || noticeDigest(files[0].text) !== entry.sha256) {
      throw new Error(heading + ': missing or altered ' + entry.name);
    }
    checked.push(entry.crate);
  }
  return checked;
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

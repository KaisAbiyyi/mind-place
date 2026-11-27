/**
 * MP-R1-WP02 command line for the single local read-only setup source (scripts/lib/mcp-local-read-setup.mjs).
 *
 *   node scripts/mcp-local-read-setup.mjs print <claude-code|antigravity> [--bridge <path>]
 *   node scripts/mcp-local-read-setup.mjs write <claude-code|antigravity> --dir <project-folder> [--bridge <path>]
 *   node scripts/mcp-local-read-setup.mjs guides [--check]
 *
 * `write` never overwrites an existing file and never writes a key: the configs only name the environment
 * variable (MIND_PLACE_MCP_TOKEN) that the user sets in their own shell.
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { GUIDE_FILES, LOCAL_READ_CLIENTS, assertReadOnlySetup, localReadSetup, renderGuide } from './lib/mcp-local-read-setup.mjs';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

export function defaultBridgePath(env = process.env) {
  return env.LOCALAPPDATA ? path.join(env.LOCALAPPDATA, 'MindPlace', 'bin', 'mcp-stdio-bridge.mjs') : null;
}

function option(args, name) {
  const at = args.indexOf(name);
  if (at < 0) return undefined;
  const value = args[at + 1];
  if (!value || value.startsWith('--')) throw new Error(`${name} requires a value`);
  return value;
}

function setupFor(client, args) {
  if (!LOCAL_READ_CLIENTS.includes(client)) throw new Error(`Client must be one of: ${LOCAL_READ_CLIENTS.join(', ')}`);
  const opts = {};
  if (client === 'antigravity') {
    const bridge = option(args, '--bridge') ?? defaultBridgePath();
    if (!bridge || !path.isAbsolute(bridge)) throw new Error('Antigravity needs an absolute --bridge path to mcp-stdio-bridge.mjs');
    opts.bridgePath = bridge;
  }
  return assertReadOnlySetup(localReadSetup(client, opts));
}

/** Writes the setup files under dir; refuses if any already exists (nothing is written in that case). */
export function writeSetup(setup, dir) {
  const targets = Object.entries(setup.files).map(([rel, body]) => [path.join(dir, rel), body]);
  const existing = targets.filter(([file]) => fs.existsSync(file)).map(([file]) => file);
  if (existing.length) throw new Error(`Not overwriting existing file(s): ${existing.join(', ')}. Add the entries from "print" by hand.`);
  for (const [file, body] of targets) {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, `${JSON.stringify(body, null, 2)}\n`, { flag: 'wx' });
  }
  return targets.map(([file]) => file);
}

/** Returns the guide files whose committed text differs from the generated text. */
export function guideDrift(root = repoRoot) {
  return Object.entries(GUIDE_FILES).filter(([lang, rel]) => {
    const file = path.join(root, rel);
    return !fs.existsSync(file) || fs.readFileSync(file, 'utf8').replace(/\r\n/g, '\n') !== renderGuide(lang);
  }).map(([, rel]) => rel);
}

function main(argv) {
  const [command, client, ...rest] = argv;
  if (command === 'print') {
    const setup = setupFor(client, rest);
    console.log(JSON.stringify({ client: setup.client, files: setup.files, launchArgs: setup.launchArgs ?? [], tokenEnv: setup.tokenEnv }, null, 2));
  } else if (command === 'write') {
    const dir = option(rest, '--dir');
    if (!dir) throw new Error('write needs --dir <project-folder>');
    const setup = setupFor(client, rest);
    if (setup.client === 'antigravity' && !fs.existsSync(setup.files['.agents/mcp_config.json'].mcpServers[setup.serverName].args[0])) {
      throw new Error('The stdio bridge is not at the --bridge path yet; copy and verify it first (see the guide).');
    }
    for (const file of writeSetup(setup, path.resolve(dir))) console.log(`wrote ${file}`);
    console.log(`Set ${setup.tokenEnv} in this shell (never in a file), then start the client from this shell.`);
  } else if (command === 'guides') {
    const check = [client, ...rest].includes('--check');
    if (check) {
      const drift = guideDrift();
      if (drift.length) throw new Error(`Setup guides are out of date: ${drift.join(', ')}. Run: node scripts/mcp-local-read-setup.mjs guides`);
      console.log('Setup guides match the setup source.');
    } else {
      for (const [lang, rel] of Object.entries(GUIDE_FILES)) {
        const file = path.join(repoRoot, rel);
        fs.mkdirSync(path.dirname(file), { recursive: true });
        fs.writeFileSync(file, renderGuide(lang));
        console.log(`wrote ${rel}`);
      }
    }
  } else {
    console.error('Usage: mcp-local-read-setup.mjs print <client> [--bridge <path>] | write <client> --dir <folder> [--bridge <path>] | guides [--check]');
    process.exitCode = 2;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(String(error?.message ?? error));
    process.exitCode = 1;
  }
}

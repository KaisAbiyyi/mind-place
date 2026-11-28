/**
 * MP-R1-WP02: the ONE setup source for local read-only Mind Place access from Claude Code and Antigravity.
 *
 * Everything that tells a local client how to reach Mind Place is generated here:
 * - the owner-facing configs (`scripts/mcp-local-read-setup.mjs print|write`),
 * - the EN/ID setup guides (`docs/setup/mcp-local-read.{en,id}.md`, rendered by `renderGuide`),
 * - the conformance drivers (`scripts/mcp-conformance/clients.mjs`), so a parity cell exercises the exact
 *   configuration a user is told to install.
 *
 * Invariants (checked by `readOnlySetupProblems` and by test/mcp-conformance-framework.test.mjs):
 * - the client may call exactly the three read tools, named one by one (no wildcard, no whole-server rule);
 * - nothing auto-approves on the provider side (no trust/auto-approve/bypass/skip-permissions option, no
 *   "enable every project server" switch);
 * - Antigravity tools stay lazy: no per-tool `eager` declaration (MP-7MCP-06);
 * - the bearer key is referenced by environment variable name only, never written into a file, a URL or an
 *   argument list; the endpoint is plain-HTTP loopback on 127.0.0.1 with no query string or embedded user.
 *
 * What a client may actually read is still decided only by Mind Place grants ("Choose chats").
 * This module has no side effects on import.
 */
import { validateLoopbackUrl } from '../mcp-stdio-bridge.mjs';

export const READ_TOOLS = Object.freeze(['memory_search', 'memory_fetch', 'context_build']);
export const LOCAL_READ_CLIENTS = Object.freeze(['claude-code', 'antigravity']);
export const SETUP_DEFAULTS = Object.freeze({
  serverName: 'mindplace_local',
  url: 'http://127.0.0.1:43731/mcp',
  tokenEnv: 'MIND_PLACE_MCP_TOKEN',
  command: 'node',
});

export const CLAUDE_CODE_FILES = Object.freeze({ mcp: '.mcp.json', settings: '.claude/settings.local.json' });
export const ANTIGRAVITY_FILES = Object.freeze({ mcp: '.agents/mcp_config.json', settings: '.agents/mind-place-read.settings.json' });

const SERVER_NAME_RE = /^[a-z][a-z0-9_]{0,31}$/;
const ENV_NAME_RE = /^[A-Za-z_][A-Za-z0-9_]*$/;

function normalize(options = {}) {
  const o = { ...SETUP_DEFAULTS, ...Object.fromEntries(Object.entries(options).filter(([, v]) => v !== undefined)) };
  if (!SERVER_NAME_RE.test(o.serverName)) throw new Error('serverName must be lower-case letters, digits or underscore (max 32), starting with a letter');
  if (!ENV_NAME_RE.test(o.tokenEnv)) throw new Error('tokenEnv must be an environment variable name');
  const url = validateLoopbackUrl(o.url);
  // The Mind Place listener accepts only Host 127.0.0.1:<port>; localhost or [::1] would be refused.
  if (url.hostname !== '127.0.0.1') throw new Error('url must use 127.0.0.1');
  if (typeof o.command !== 'string' || o.command === '') throw new Error('command must be a non-empty string');
  return { ...o, url: url.href };
}

export function claudeCodeToolNames(serverName = SETUP_DEFAULTS.serverName) {
  return READ_TOOLS.map((tool) => `mcp__${serverName}__${tool}`);
}

export function antigravityToolRules(serverName = SETUP_DEFAULTS.serverName) {
  return READ_TOOLS.map((tool) => `mcp(${serverName}/${tool})`);
}

/**
 * Claude Code: project `.mcp.json` (Streamable HTTP, header value expanded by Claude Code from the named
 * environment variable) plus a local project settings file that allows the three read tools by exact name.
 * Claude Code still asks once before it uses a project server; nothing here pre-approves it.
 */
export function claudeCodeSetup(options) {
  const o = normalize(options);
  const allowedTools = claudeCodeToolNames(o.serverName);
  return {
    client: 'claude-code',
    serverName: o.serverName,
    tokenEnv: o.tokenEnv,
    allowedTools,
    files: {
      [CLAUDE_CODE_FILES.mcp]: {
        mcpServers: { [o.serverName]: { type: 'http', url: o.url, headers: { Authorization: `Bearer \${${o.tokenEnv}}` } } },
      },
      [CLAUDE_CODE_FILES.settings]: { permissions: { allow: [...allowedTools] } },
    },
  };
}

/**
 * Antigravity: workspace `.agents/mcp_config.json` launching the loopback stdio bridge (Antigravity does not
 * expand environment variables in HTTP headers, so the bridge reads the key from the named variable), plus a
 * session-only settings file passed with `--settings` that allows the three read tools by exact name.
 * No per-tool block at all: tools stay lazy (MP-7MCP-06).
 */
export function antigravitySetup(options = {}) {
  if (typeof options.bridgePath !== 'string' || options.bridgePath === '') throw new Error('bridgePath is required for Antigravity');
  const o = normalize(options);
  const allow = antigravityToolRules(o.serverName);
  return {
    client: 'antigravity',
    serverName: o.serverName,
    tokenEnv: o.tokenEnv,
    allowedTools: allow,
    launchArgs: ['--settings', ANTIGRAVITY_FILES.settings],
    files: {
      [ANTIGRAVITY_FILES.mcp]: {
        mcpServers: { [o.serverName]: { command: o.command, args: [o.bridgePath, '--url', o.url, '--token-env', o.tokenEnv] } },
      },
      [ANTIGRAVITY_FILES.settings]: { permissions: { allow: [...allow] } },
    },
  };
}

export function localReadSetup(client, options) {
  if (client === 'claude-code') return claudeCodeSetup(options);
  if (client === 'antigravity') return antigravitySetup(options);
  throw new Error(`Unknown local read client: ${client} (expected ${LOCAL_READ_CLIENTS.join(' or ')})`);
}

// Keys that would hand approval to the provider, widen the tool set, or reintroduce eager declarations.
const FORBIDDEN_KEYS = new Set([
  'eager', 'tools', 'trust', 'autoapprove', 'alwaysallow', 'autoallow', 'includetools', 'excludetools',
  'enableallprojectmcpservers', 'enabledmcpjsonservers', 'defaultmode', 'dangerouslyskippermissions',
  'skipdangerousmodepermissionprompt', 'bypasspermissions', 'yolo',
]);
const FORBIDDEN_VALUES = [
  [/\*/, 'wildcard'],
  [/dangerously|bypassPermissions|skip-permissions|--yolo/i, 'auto-approve flag'],
  [/mpc_v1_[0-9A-Za-z]/, 'literal Mind Place key'],
  [/Bearer\s+(?!\$\{[A-Za-z_][A-Za-z0-9_]*\}$)/, 'literal bearer value'],
  [/--(token|bearer|header|authorization|secret|key)\b/i, 'inline credential flag'],
];

function walk(value, at, visit) {
  visit(value, at);
  if (Array.isArray(value)) value.forEach((v, i) => walk(v, `${at}[${i}]`, visit));
  else if (value && typeof value === 'object') for (const [k, v] of Object.entries(value)) walk(v, `${at}.${k}`, visit);
}

/** Returns a list of problems (empty when the setup is read-only as required). */
export function readOnlySetupProblems(setup) {
  const problems = [];
  if (!setup || !LOCAL_READ_CLIENTS.includes(setup.client)) return ['unknown client'];
  const expected = setup.client === 'claude-code' ? claudeCodeToolNames(setup.serverName) : antigravityToolRules(setup.serverName);
  const files = setup.files ?? {};
  for (const [file, body] of Object.entries(files)) {
    walk(body, file, (value, at) => {
      if (value && typeof value === 'object' && !Array.isArray(value)) {
        for (const key of Object.keys(value)) if (FORBIDDEN_KEYS.has(key.toLowerCase())) problems.push(`${at}: forbidden key "${key}"`);
      }
      if (typeof value === 'string') {
        for (const [re, why] of FORBIDDEN_VALUES) {
          if (re.test(value) && !(why === 'inline credential flag' && value === '--token-env')) problems.push(`${at}: ${why}`);
        }
      }
    });
  }
  const settingsFile = setup.client === 'claude-code' ? CLAUDE_CODE_FILES.settings : ANTIGRAVITY_FILES.settings;
  const mcpFile = setup.client === 'claude-code' ? CLAUDE_CODE_FILES.mcp : ANTIGRAVITY_FILES.mcp;
  const allow = files[settingsFile]?.permissions?.allow;
  if (JSON.stringify(allow) !== JSON.stringify(expected)) problems.push(`${settingsFile}: allow list must be exactly ${expected.join(', ')}`);
  if (JSON.stringify(setup.allowedTools) !== JSON.stringify(expected)) problems.push('allowedTools must be exactly the three read tools');
  const servers = files[mcpFile]?.mcpServers ?? {};
  if (Object.keys(servers).length !== 1 || !servers[setup.serverName]) problems.push(`${mcpFile}: exactly one server named ${setup.serverName}`);
  const server = servers[setup.serverName] ?? {};
  try {
    if (setup.client === 'claude-code') {
      if (server.type !== 'http') problems.push(`${mcpFile}: type must be http`);
      if (validateLoopbackUrl(server.url).hostname !== '127.0.0.1') problems.push(`${mcpFile}: url must use 127.0.0.1`);
      if (server.headers?.Authorization !== `Bearer \${${setup.tokenEnv}}`) problems.push(`${mcpFile}: Authorization must reference \${${setup.tokenEnv}}`);
    } else {
      const args = server.args ?? [];
      const urlAt = args.indexOf('--url');
      const envAt = args.indexOf('--token-env');
      if (urlAt < 0 || envAt < 0 || args.length !== 5) problems.push(`${mcpFile}: args must be <bridge> --url <url> --token-env <name>`);
      else {
        if (validateLoopbackUrl(args[urlAt + 1]).hostname !== '127.0.0.1') problems.push(`${mcpFile}: url must use 127.0.0.1`);
        if (args[envAt + 1] !== setup.tokenEnv) problems.push(`${mcpFile}: --token-env must be ${setup.tokenEnv}`);
      }
      if (!/mcp-stdio-bridge\.mjs$/.test(String(args[0] ?? ''))) problems.push(`${mcpFile}: first argument must be the Mind Place stdio bridge`);
    }
  } catch (error) {
    problems.push(`${mcpFile}: ${error.message}`);
  }
  return problems;
}

export function assertReadOnlySetup(setup) {
  const problems = readOnlySetupProblems(setup);
  if (problems.length) throw new Error(`Setup is not read-only: ${problems.join('; ')}`);
  return setup;
}

// ---------------------------------------------------------------------------------------------------------
// EN/ID guides. Rendered from the same setup functions, so the JSON a user copies is the JSON the drivers use.
// UI words match the app catalogs (src/ui2/i18n/catalog-en.ts and catalog-id.ts).

export const GUIDE_FILES = Object.freeze({ en: 'docs/setup/mcp-local-read.en.md', id: 'docs/setup/mcp-local-read.id.md' });
export const GUIDE_BRIDGE_PLACEHOLDER = 'C:\\Users\\<you>\\AppData\\Local\\MindPlace\\bin\\mcp-stdio-bridge.mjs';

const T = {
  en: {
    title: 'Let Claude Code or Antigravity read your Mind Place memory',
    generated: 'Generated by `node scripts/mcp-local-read-setup.mjs guides` from `scripts/lib/mcp-local-read-setup.mjs`. Do not edit by hand. Indonesian: [mcp-local-read.id.md](mcp-local-read.id.md).',
    whatH: 'What this sets up',
    what: [
      'One of these coding tools on this computer can **search and read** the chats you allow. It can never change anything.',
      'It gets exactly three read tools, named one by one: {tools}. No wildcard, no automatic approval, nothing else.',
      'Which chats it can read is decided only in Mind Place, under **Choose chats**. You can turn it off at any time.',
      'This guide covers Claude Code and Antigravity only. The Claude app, Gemini CLI and hosted apps are set up differently.',
    ],
    beforeH: 'Before you start',
    before: [
      'Open Mind Place and unlock it. Leave it open while you use the tool.',
      'Go to **Connections** and turn on **Share memory with AI apps**.',
      'Under **Name of the app** type the tool name (for example `Claude Code`), then press **Create access**.',
      '**Copy your key now**: it is shown only once. Do not paste it into a chat, a file, a command or a screenshot.',
      'Press **Choose chats** for that app and allow only the chats it should read.',
    ],
    keyH: 'Give the key to this window only',
    key: 'Open PowerShell 7 in the project folder you will work in. Paste the key when asked; it is not shown and is not saved in your command history. The key lasts only as long as this window.',
    keyAfter: 'Start the tool from **this same window**. If you close it, repeat this step.',
    ccH: 'Claude Code',
    cc1: 'From a Mind Place checkout, write the two files into your project folder (existing files are never overwritten):',
    cc2: 'This creates the files below. If you already have one of them, add these entries to it by hand instead.',
    cc3: 'Start Claude Code in that folder. When it asks whether to use the project server `{server}`, say yes. The three tools then run without further prompts; any other tool still asks.',
    agH: 'Antigravity',
    ag0: 'Antigravity cannot read the key from a setting, so it talks to Mind Place through a small helper that reads the key from this window only. Copy `scripts/mcp-stdio-bridge.mjs` from the same Mind Place version into `%LOCALAPPDATA%\\MindPlace\\bin\\` and check that its SHA-256 matches that version. It needs Node.js 20 or newer.',
    ag1: 'Write the two files into your project folder (existing files are never overwritten):',
    ag2: 'This creates the files below (your own user name instead of `<you>`).',
    ag3: 'Start Antigravity in that folder with the read-only settings for this session:',
    dontH: 'Never do this',
    dont: [
      'Never put the key in a file, a link, a command line or a screenshot.',
      'Never allow a whole server or a wildcard (`mcp__{server}`, `mcp__*`, `mcp(*)`).',
      'Never start the tool with an option that approves everything (`--dangerously-skip-permissions`, bypass or auto-approve modes).',
      'Never mark Antigravity tools as `eager`; the larger read tools then stop working.',
    ],
    checkH: 'Check that it works',
    check: 'Ask the tool: "Use only the {server} tools. Search Mind Place for a word from a chat you allowed." It should find that chat, and nothing from chats you did not allow.',
    offH: 'Turn it off',
    off: 'In **Connections**, press **Revoke access** for that app. It stops working right away. Locking Mind Place also stops all access until you unlock and share again.',
  },
  id: {
    title: 'Izinkan Claude Code atau Antigravity membaca memori Mind Place-mu',
    generated: 'Dibuat oleh `node scripts/mcp-local-read-setup.mjs guides` dari `scripts/lib/mcp-local-read-setup.mjs`. Jangan diedit manual. English: [mcp-local-read.en.md](mcp-local-read.en.md).',
    whatH: 'Apa yang disiapkan',
    what: [
      'Salah satu alat coding ini di komputer ini bisa **mencari dan membaca** obrolan yang kamu izinkan. Alat itu tidak pernah bisa mengubah apa pun.',
      'Alat itu mendapat tepat tiga alat baca, disebut satu per satu: {tools}. Tanpa wildcard, tanpa persetujuan otomatis, tidak ada yang lain.',
      'Obrolan mana yang boleh dibaca hanya ditentukan di Mind Place, lewat **Pilih obrolan**. Kamu bisa mematikannya kapan saja.',
      'Panduan ini hanya untuk Claude Code dan Antigravity. Aplikasi Claude, Gemini CLI dan aplikasi hosted disiapkan dengan cara lain.',
    ],
    beforeH: 'Sebelum mulai',
    before: [
      'Buka Mind Place dan buka kuncinya. Biarkan terbuka selama alat dipakai.',
      'Buka **Koneksi** lalu nyalakan **Bagikan memori ke aplikasi AI**.',
      'Di **Nama aplikasi** ketik nama alatnya (misalnya `Claude Code`), lalu tekan **Buat akses**.',
      '**Salin kuncimu sekarang**: kunci hanya ditampilkan sekali. Jangan tempel ke obrolan, file, perintah, atau tangkapan layar.',
      'Tekan **Pilih obrolan** untuk aplikasi itu dan izinkan hanya obrolan yang boleh dibaca.',
    ],
    keyH: 'Berikan kunci hanya ke jendela ini',
    key: 'Buka PowerShell 7 di folder proyek yang akan kamu pakai. Tempel kunci saat diminta; kunci tidak ditampilkan dan tidak tersimpan di riwayat perintah. Kunci hanya berlaku selama jendela ini terbuka.',
    keyAfter: 'Jalankan alatnya dari **jendela yang sama**. Kalau jendela ditutup, ulangi langkah ini.',
    ccH: 'Claude Code',
    cc1: 'Dari salinan Mind Place, tulis dua file ke folder proyekmu (file yang sudah ada tidak pernah ditimpa):',
    cc2: 'Perintah ini membuat file di bawah. Kalau salah satunya sudah ada, tambahkan isian ini secara manual.',
    cc3: 'Jalankan Claude Code di folder itu. Saat ditanya apakah memakai server proyek `{server}`, jawab ya. Tiga alat itu lalu berjalan tanpa ditanya lagi; alat lain tetap meminta izin.',
    agH: 'Antigravity',
    ag0: 'Antigravity tidak bisa membaca kunci dari pengaturan, jadi ia terhubung ke Mind Place lewat pembantu kecil yang membaca kunci hanya dari jendela ini. Salin `scripts/mcp-stdio-bridge.mjs` dari versi Mind Place yang sama ke `%LOCALAPPDATA%\\MindPlace\\bin\\` dan pastikan SHA-256-nya cocok dengan versi itu. Butuh Node.js 20 atau lebih baru.',
    ag1: 'Tulis dua file ke folder proyekmu (file yang sudah ada tidak pernah ditimpa):',
    ag2: 'Perintah ini membuat file di bawah (dengan nama penggunamu sendiri sebagai ganti `<you>`).',
    ag3: 'Jalankan Antigravity di folder itu dengan pengaturan baca-saja untuk sesi ini:',
    dontH: 'Jangan pernah',
    dont: [
      'Jangan pernah menaruh kunci di file, tautan, baris perintah, atau tangkapan layar.',
      'Jangan pernah mengizinkan seluruh server atau wildcard (`mcp__{server}`, `mcp__*`, `mcp(*)`).',
      'Jangan pernah menjalankan alat dengan opsi yang menyetujui semuanya (`--dangerously-skip-permissions`, mode bypass atau auto-approve).',
      'Jangan pernah menandai alat Antigravity sebagai `eager`; alat baca yang lebih besar jadi tidak berfungsi.',
    ],
    checkH: 'Cek apakah berfungsi',
    check: 'Minta ke alatnya: "Pakai hanya alat {server}. Cari di Mind Place sebuah kata dari obrolan yang kamu izinkan." Obrolan itu harus ketemu, dan tidak ada apa pun dari obrolan yang tidak kamu izinkan.',
    offH: 'Matikan',
    off: 'Di **Koneksi**, tekan **Cabut akses** untuk aplikasi itu. Akses langsung berhenti. Mengunci Mind Place juga menghentikan semua akses sampai kamu membuka kunci dan berbagi lagi.',
  },
};

const fence = (lang, body) => `\`\`\`${lang}\n${body}\n\`\`\``;
const json = (value) => fence('json', JSON.stringify(value, null, 2));

export function renderGuide(lang) {
  const t = T[lang];
  if (!t) throw new Error(`No guide language ${lang}`);
  const cc = assertReadOnlySetup(claudeCodeSetup());
  const ag = assertReadOnlySetup(antigravitySetup({ bridgePath: GUIDE_BRIDGE_PLACEHOLDER }));
  const fill = (s) => s.replaceAll('{server}', SETUP_DEFAULTS.serverName).replaceAll('{tools}', READ_TOOLS.map((n) => `\`${n}\``).join(', '));
  const list = (items, numbered) => items.map((s, i) => `${numbered ? `${i + 1}.` : '-'} ${fill(s)}`).join('\n');
  const files = (setup) => Object.entries(setup.files).map(([name, body]) => `\`${name}\`\n\n${json(body)}`).join('\n\n');
  return [
    `# ${t.title}`,
    `> ${t.generated}`,
    `## ${t.whatH}`, list(t.what),
    `## ${t.beforeH}`, list(t.before, true),
    `## ${t.keyH}`, t.key,
    fence('powershell', `$env:${SETUP_DEFAULTS.tokenEnv} = Read-Host -MaskInput 'Mind Place key'`),
    t.keyAfter,
    `## ${t.ccH}`, t.cc1,
    fence('powershell', 'node scripts/mcp-local-read-setup.mjs write claude-code --dir <project-folder>'),
    t.cc2, files(cc), fill(t.cc3),
    fence('powershell', 'claude'),
    `## ${t.agH}`, t.ag0, t.ag1,
    fence('powershell', 'node scripts/mcp-local-read-setup.mjs write antigravity --dir <project-folder>'),
    t.ag2, files(ag), t.ag3,
    fence('powershell', `agy ${ag.launchArgs.join(' ')}`),
    `## ${t.dontH}`, list(t.dont),
    `## ${t.checkH}`, fill(t.check),
    `## ${t.offH}`, t.off,
  ].join('\n\n') + '\n';
}

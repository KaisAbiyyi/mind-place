/**
 * Parity Matrix model for Release 1 read-only MCP interoperability.
 *
 * A cell is PASS only with evidence that a REAL provider client did the thing
 * (observed on the wire by the recording proxy, with its own announced client
 * name and version) on one exact source SHA. A synthetic HTTP client is tracked
 * as its own reference row and can never turn a provider row green.
 */
import { containsSecret } from '../lib/e2e-secret-guard.mjs';

export const RESULTS = ['PASS', 'FAIL', 'NOT_RUN', 'PROVIDER_BLOCKED'];
export const STORE_MODE = 'encrypted';

export const CLAIMS = [
  { id: 'C1', title: 'Handshake and initialization', detail: 'Real initialize (or discovery) accepted; protocol negotiated; client identity observed on the wire' },
  { id: 'C2', title: 'Tool discovery', detail: 'tools/list returns exactly the three read-only tools; client accepts their schemas' },
  { id: 'C3', title: 'Authenticated, scoped search', detail: 'memory_search returns hits from granted chats only' },
  { id: 'C4', title: 'Exact full-content fetch', detail: 'A conversation read byte-exact across cursor pages; no silent truncation. Measured scope differs by row: the synthetic reference reads a long (over 256 KB, multi-revision, multi-byte) chat; a real client reads the medium multi-byte two-revision chat in small pages (a long-chat result stream does not fit a CLI model\'s context). Real-client long-chat read is NOT_RUN and is stated in each cell\'s scope.' },
  { id: 'C5', title: 'Revisions, identity, provenance', detail: 'Historical revisions fetchable; stable source/snapshot ids, provider and timestamps preserved' },
  { id: 'C6', title: 'Default deny and isolation', detail: 'Nothing is readable before a grant, and a denied fetch looks like an unknown ID. Real clients exercise their granted scope. The synthetic reference also checks two distinct credentials with disjoint grants.' },
  { id: 'C7', title: 'Immediate revocation', detail: 'Chat grant and whole-client revocation take effect on the next request' },
  { id: 'C8', title: 'Lock enforcement', detail: 'After Lock the listener refuses or closes; nothing is disclosed' },
  { id: 'C9', title: 'Reconnect without escalation', detail: 'After a dropped connection or restart the client regains only what it was granted' },
];

/** Rows. `synthetic: true` rows can only ever carry SYNTHETIC_HTTP evidence. */
export const CLIENTS = [
  { id: 'synthetic-http', label: 'Synthetic HTTP reference (not a provider)', surface: 'harness', transport: 'streamable-http-loopback', synthetic: true },
  { id: 'claude-code', label: 'Claude Code', surface: 'local CLI', transport: 'streamable-http-loopback' },
  { id: 'codex', label: 'Codex', surface: 'local CLI', transport: 'streamable-http-loopback' },
  { id: 'gemini-cli', label: 'Gemini CLI', surface: 'local CLI', transport: 'streamable-http-loopback' },
  { id: 'antigravity', label: 'Antigravity', surface: 'local CLI', transport: 'stdio-bridge -> streamable-http-loopback' },
  { id: 'claude-app', label: 'Claude (desktop / web app)', surface: 'hosted or desktop app', transport: 'custom connector (HTTPS)' },
  { id: 'chatgpt', label: 'ChatGPT', surface: 'hosted', transport: 'secure tunnel (HTTPS)' },
  { id: 'gemini-app', label: 'Gemini (hosted app)', surface: 'hosted', transport: 'custom connector (HTTPS)' },
];

const SHA = /^[0-9a-f]{40}$/;
const HEX64 = /^[0-9a-f]{64}$/;

/** Returns a list of problems; empty means the cell is internally honest. */
export function validateCell(clientId, claimId, cell) {
  const problems = [];
  const where = `${clientId}/${claimId}`;
  const client = CLIENTS.find((c) => c.id === clientId);
  if (!client) return [`${where}: unknown client`];
  if (!CLAIMS.some((c) => c.id === claimId)) return [`${where}: unknown claim`];
  if (!cell || !RESULTS.includes(cell.result)) return [`${where}: result must be one of ${RESULTS.join(', ')}`];
  if (containsSecret(JSON.stringify(cell))) problems.push(`${where}: cell contains a secret-shaped value`);
  const kind = cell.evidence?.kind ?? null;
  if (cell.result === 'PASS' || cell.result === 'FAIL') {
    if (cell.store_mode !== STORE_MODE) problems.push(`${where}: executed cells require verified encrypted-vault evidence`);
    if (!cell.sha || !SHA.test(cell.sha)) problems.push(`${where}: ${cell.result} needs the exact 40-hex source SHA`);
    if (!cell.evidence?.receipt || !HEX64.test(cell.evidence.receipt_sha256 ?? '')) problems.push(`${where}: needs a receipt file and its sha256`);
    if (client.synthetic) {
      if (kind !== 'SYNTHETIC_HTTP') problems.push(`${where}: the synthetic row may only carry SYNTHETIC_HTTP evidence`);
    } else {
      if (kind !== 'REAL_CLIENT') problems.push(`${where}: a provider row needs REAL_CLIENT evidence, not ${kind ?? 'none'}`);
      const observed = cell.evidence?.observed_client;
      if (!observed?.name || !observed?.version) problems.push(`${where}: real-client evidence needs the client name and version observed on the wire`);
      if (!cell.evidence?.transport) problems.push(`${where}: needs the transport`);
      if (claimId === 'C4' && !String(cell.scope ?? '').trim()) problems.push(`${where}: a real-client C4 cell must state its measured scope (what content size and revisions were actually read)`);
    }
  }
  if (cell.result === 'PROVIDER_BLOCKED') {
    if (!cell.blocker) problems.push(`${where}: PROVIDER_BLOCKED needs the vendor restriction as evidence`);
    if (!cell.fallback) problems.push(`${where}: PROVIDER_BLOCKED needs the fallback surface`);
  }
  if (cell.result === 'NOT_RUN') {
    if (!cell.reason) problems.push(`${where}: NOT_RUN needs the reason`);
    if (!client.synthetic && !cell.owner_card) problems.push(`${where}: NOT_RUN on a provider row needs an executable owner test card`);
  }
  return problems;
}

export function validateMatrix(matrix) {
  const problems = [];
  if (!matrix?.sha || !SHA.test(matrix.sha)) problems.push('matrix: exact source SHA missing');
  if (matrix?.store_mode !== STORE_MODE) problems.push('matrix: encrypted vault store mode required');
  for (const client of CLIENTS) {
    for (const claim of CLAIMS) {
      const cell = matrix?.cells?.[client.id]?.[claim.id];
      if (!cell) { problems.push(`${client.id}/${claim.id}: cell missing (every cell must be explicit)`); continue; }
      problems.push(...validateCell(client.id, claim.id, cell));
      // One frozen version: every executed cell ran at the matrix commit, with the same conformance harness and product.
      if ((cell.result === 'PASS' || cell.result === 'FAIL') && matrix.sha && cell.sha && cell.sha !== matrix.sha) {
        problems.push(`${client.id}/${claim.id}: cell ran at ${cell.sha.slice(0, 9)} but the matrix is for ${matrix.sha.slice(0, 9)} (rerun every cell at one frozen commit)`);
      }
      if ((cell.result === 'PASS' || cell.result === 'FAIL') && matrix.harness_tree_id && cell.harness_tree_id !== matrix.harness_tree_id) {
        problems.push(`${client.id}/${claim.id}: cell was run with a different conformance harness than the matrix`);
      }
      if (cell.product_tree_id && matrix.product_tree_id && cell.product_tree_id !== matrix.product_tree_id) {
        problems.push(`${client.id}/${claim.id}: cell was run on a different product (tree ${String(cell.product_tree_id).slice(0, 8)}) than the matrix (${String(matrix.product_tree_id).slice(0, 8)})`);
      }
      if (cell.executable_sha256 && matrix.executable_sha256 && cell.executable_sha256 !== matrix.executable_sha256) {
        problems.push(`${client.id}/${claim.id}: cell was run on a different executable`);
      }
    }
  }
  return problems;
}

const mark = { PASS: 'PASS', FAIL: 'FAIL', NOT_RUN: 'NOT_RUN', PROVIDER_BLOCKED: 'PROVIDER_BLOCKED' };

export function renderMatrixMarkdown(matrix) {
  const lines = [];
  lines.push('# Release 1 read-only MCP Parity Matrix');
  lines.push('');
  lines.push(`Source SHA: \`${matrix.sha}\` · generated ${matrix.generated_at}`);
  lines.push('');
  lines.push(`Product under test: mind-place.exe sha256 \`${matrix.executable_sha256}\`, product source tree id \`${matrix.product_tree_id}\`, conformance harness tree id \`${matrix.harness_tree_id ?? 'n/a'}\`. Every PASS or FAIL cell ran at exactly this commit with exactly this harness and product (enforced by the validator). The commit that records this matrix adds only the documentation and receipts under \`docs/qualification/mcp-parity/\`.`);
  lines.push('');
  lines.push('Store under test: packaged encrypted vault populated with disposable synthetic captures.');
  lines.push('');
  lines.push('Legend: PASS = a REAL provider client did it on this exact SHA (client name and version observed on the wire). The synthetic row is a server reference, never provider proof. NOT_RUN and PROVIDER_BLOCKED are explicit, not hidden.');
  lines.push('');
  lines.push(['Client', 'Transport', ...CLAIMS.map((c) => c.id)].join(' | ').replace(/^/, '| ').concat(' |'));
  lines.push(`|${' --- |'.repeat(CLAIMS.length + 2)}`);
  for (const client of CLIENTS) {
    const cells = CLAIMS.map((claim) => mark[matrix.cells?.[client.id]?.[claim.id]?.result] ?? 'MISSING');
    lines.push(`| ${client.label} | ${client.transport} | ${cells.join(' | ')} |`);
  }
  lines.push('');
  lines.push('## Claims');
  for (const claim of CLAIMS) lines.push(`- **${claim.id} ${claim.title}**: ${claim.detail}`);
  lines.push('');
  lines.push('## Cell notes');
  for (const client of CLIENTS) {
    for (const claim of CLAIMS) {
      const cell = matrix.cells?.[client.id]?.[claim.id];
      if (!cell) continue;
      const bits = [];
      if (cell.evidence?.observed_client) bits.push(`client ${cell.evidence.observed_client.name} ${cell.evidence.observed_client.version}`);
      if (cell.evidence?.transport) bits.push(`transport ${cell.evidence.transport}`);
      if (cell.evidence?.receipt) bits.push(`receipt ${cell.evidence.receipt}`);
      if (cell.scope) bits.push(`scope: ${cell.scope}`);
      if (cell.note) bits.push(cell.note);
      if (cell.reason) bits.push(`reason: ${cell.reason}`);
      if (cell.blocker) bits.push(`blocker: ${cell.blocker}; fallback: ${cell.fallback}`);
      if (cell.owner_card) bits.push(`owner card: ${cell.owner_card}`);
      lines.push(`- ${client.label} / ${claim.id} — ${cell.result}${bits.length ? ': ' + bits.join('; ') : ''}`);
    }
  }
  return lines.join('\n') + '\n';
}

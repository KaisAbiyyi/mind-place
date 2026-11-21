import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  canonicalSourceIds,
  grantMcpClientSources,
  listMcpClientSources,
  MAX_SOURCES_PER_REQUEST,
  revokeAllMcpClientSources,
  revokeMcpClientSources,
} from '../src/mcp-read-grants.js';
import type { Invoke } from '../src/reader.js';

const id = (n: number) => `src:v1:${n.toString(16).padStart(64, '0')}`;

function recorder() {
  const calls: Array<{ command: string; args: Record<string, unknown> }> = [];
  const invoke: Invoke = async <T>(command: string, args: Record<string, unknown> = {}) => {
    calls.push({ command, args });
    const ids = (args.sourceIds as string[] | undefined) ?? [];
    return (command === 'list_local_mcp_client_sources' ? [id(1)] : ids.length) as T;
  };
  return { calls, invoke };
}

test('source ids must be canonical; wildcards and look-alikes are rejected before any native call', async () => {
  for (const bad of ['*', 'src:v1:*', '', 'src:v1:' + 'G'.repeat(64), 'src:v1:' + '0'.repeat(63), 'SRC:V1:' + '0'.repeat(64), id(1).toUpperCase()]) {
    assert.throws(() => canonicalSourceIds([bad]), /Invalid source id/);
  }
  const { calls, invoke } = recorder();
  await assert.rejects(() => grantMcpClientSources(invoke, 'client:v1:a', [id(1), '*']), /Invalid source id/);
  assert.equal(calls.length, 0, 'nothing is sent when any id is invalid');
});

test('grants are de-duplicated and chunked to the native per-call limit', async () => {
  const { calls, invoke } = recorder();
  const ids = Array.from({ length: MAX_SOURCES_PER_REQUEST * 2 + 5 }, (_, n) => id(n));
  const changed = await grantMcpClientSources(invoke, 'client:v1:a', [...ids, id(0)]);
  assert.equal(changed, ids.length);
  assert.deepEqual(calls.map(c => (c.args.sourceIds as string[]).length), [1000, 1000, 5]);
  assert.ok(calls.every(c => c.command === 'grant_local_mcp_client_sources' && c.args.clientId === 'client:v1:a'));
});

test('revoke, revoke-all and list use the owner-only commands with the client id', async () => {
  const { calls, invoke } = recorder();
  await revokeMcpClientSources(invoke, 'client:v1:a', [id(2)]);
  await revokeAllMcpClientSources(invoke, 'client:v1:a');
  assert.deepEqual(await listMcpClientSources(invoke, 'client:v1:a'), [id(1)]);
  assert.deepEqual(calls.map(c => c.command), [
    'revoke_local_mcp_client_sources',
    'revoke_all_local_mcp_client_sources',
    'list_local_mcp_client_sources',
  ]);
});

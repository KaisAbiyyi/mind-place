import assert from 'node:assert/strict';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { mountLocalMcpSetup } from '../src/mcp-setup.js';
import type { Invoke } from '../src/reader.js';

test('hosted setup explains included helper and gives repair guidance without exposing keys', async () => {
  const dom = new JSDOM('<body><main></main></body>');
  const previous = { document: globalThis.document, Element: globalThis.Element };
  Object.assign(globalThis, { document: dom.window.document, Element: dom.window.Element });
  try {
    const invoke: Invoke = async <T>(command: string) => {
      if (command === 'list_local_mcp_clients') return [] as T;
      if (command === 'start_hosted_tunnel') throw new Error('MP_TUNNEL_INTEGRITY:damaged helper');
      return { running: false, tunnel_id: null, connection_verified: false } as T;
    };
    const parent = dom.window.document.querySelector('main')!;
    mountLocalMcpSetup(parent, invoke, { writeText: async () => {} });
    assert.match(parent.textContent ?? '', /included with Mind Place/i);
    assert.doesNotMatch(parent.textContent ?? '', /install the verified official tunnel-client/i);
    const key = parent.querySelector<HTMLInputElement>('#hosted-runtime-key')!;
    key.value = 'sk-private-fixture-do-not-display';
    parent.querySelector<HTMLButtonElement>('[data-mcp-action="hosted-start"]')!.click();
    await new Promise(resolve => setTimeout(resolve, 0));
    assert.equal(key.value, '');
    assert.match(parent.textContent ?? '', /repair|reinstall/i);
    assert.doesNotMatch(parent.textContent ?? '', /sk-private-fixture/);
  } finally { Object.assign(globalThis, previous); dom.window.close(); }
});

test('disposed MCP setup ignores delayed credentials and clears password fields', async () => {
  const dom = new JSDOM('<main></main>');
  const parent = dom.window.document.querySelector('main')!;
  let finish!: (value: unknown) => void;
  const pending = new Promise(resolve => { finish = resolve; });
  const invoke: Invoke = async <T>(cmd: string) => {
    if (cmd === 'issue_local_mcp_reader') return pending as Promise<T>;
    if (cmd === 'list_local_mcp_clients') return [] as T;
    return { running: false, connection_verified: false } as T;
  };
  const handle = mountLocalMcpSetup(parent, invoke, { writeText: async () => {} });
  const key = parent.querySelector<HTMLInputElement>('#hosted-runtime-key')!; key.value = 'synthetic-private-key';
  parent.querySelector<HTMLButtonElement>('[data-mcp-action="issue"]')!.click();
  handle.destroy(); finish({ client_id: 'late', credential: 'synthetic-late-credential' });
  await new Promise(resolve => setTimeout(resolve, 0));
  assert.equal(key.value, ''); assert.doesNotMatch(parent.textContent ?? '', /synthetic-/);
  dom.window.close();
});

test('collections panel validates source IDs, grants and revokes through owner commands only', async () => {
  const dom = new JSDOM('<main></main>');
  const previous = { document: globalThis.document, Element: globalThis.Element };
  Object.assign(globalThis, { document: dom.window.document, Element: dom.window.Element });
  try {
    const calls: Array<[string, unknown]> = [];
    const sourceId = 'src:v1:' + 'a'.repeat(64);
    const collections = [{ id: 'collection:v1:' + '1'.repeat(32), name: 'Disposable synthetic', source_count: 1 }];
    const invoke: Invoke = async <T>(command: string, args?: unknown) => {
      calls.push([command, args]);
      if (command === 'list_local_mcp_clients') return [{ id: 'client:v1:r', display_name: 'Claude Code', state: 'enabled', policy_revision: 1 }] as T;
      if (command === 'list_local_mcp_collections') return collections as T;
      if (command === 'list_local_mcp_client_collections') return [] as T;
      if (command === 'create_local_mcp_collection') return collections[0].id as T;
      return { running: false, connection_verified: false } as T;
    };
    const parent = dom.window.document.querySelector('main')!;
    mountLocalMcpSetup(parent, invoke, { writeText: async () => {} });
    const tick = () => new Promise(resolve => setTimeout(resolve, 0));
    const details = parent.querySelector<HTMLDetailsElement>('[data-mcp-collections]')!;
    // Nothing collection-related is invoked until the owner opens the panel.
    assert.equal(calls.some(([c]) => c.includes('collection')), false);
    details.open = true;
    details.dispatchEvent(new dom.window.Event('toggle'));
    await tick(); await tick();
    assert.match(parent.textContent ?? '', /Disposable synthetic \(1 conversations\)/);
    assert.match(parent.textContent ?? '', /can read nothing/);

    const name = parent.querySelector<HTMLInputElement>('#mcp-collection-name')!;
    const sources = parent.querySelector<HTMLTextAreaElement>('#mcp-collection-sources')!;
    name.value = 'Fixture'; sources.value = 'not-a-source-id';
    parent.querySelector<HTMLButtonElement>('[data-mcp-action="collection-create"]')!.click();
    await tick();
    assert.equal(calls.some(([c]) => c === 'create_local_mcp_collection'), false, 'invalid IDs never reach the backend');
    sources.value = sourceId + '\n' + sourceId + '\n';
    parent.querySelector<HTMLButtonElement>('[data-mcp-action="collection-create"]')!.click();
    await tick(); await tick();
    const create = calls.find(([c]) => c === 'create_local_mcp_collection')!;
    assert.deepEqual(create[1], { name: 'Fixture', sourceIds: [sourceId] });

    parent.querySelector<HTMLButtonElement>('[data-mcp-action="collection-grant"]')!.click();
    await tick();
    assert.deepEqual(calls.find(([c]) => c === 'grant_local_mcp_collection')![1],
      { clientId: 'client:v1:r', collectionId: collections[0].id });
    parent.querySelector<HTMLButtonElement>('[data-mcp-action="collection-revoke"]')!.click();
    await tick();
    assert.ok(calls.some(([c]) => c === 'revoke_local_mcp_collection_grant'));
  } finally { Object.assign(globalThis, previous); dom.window.close(); }
});

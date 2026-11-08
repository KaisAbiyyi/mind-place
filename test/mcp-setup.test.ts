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

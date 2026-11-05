import assert from "node:assert/strict";
import { test } from "node:test";
import { setTimeout as delay } from "node:timers/promises";
import { JSDOM } from "jsdom";
import { mountProvidersWorkspace } from "../src/providers-workspace.js";
import type { Invoke } from "../src/reader.js";

test("provider capture support never implies an MCP handshake or write authority", async () => {
  const dom = new JSDOM("<main></main>"); const view = dom.window.document.querySelector("main")!;
  const calls: string[] = [];
  const invoke: Invoke = async <T>(cmd: string) => {
    calls.push(cmd);
    if (cmd === "get_local_mcp_status") return { running: true, endpoint: "http://127.0.0.1:43731/mcp" } as T;
    if (cmd === "get_hosted_tunnel_status") return { running: true, connection_verified: false, tunnel_id: "fixture" } as T;
    if (cmd === "list_local_mcp_clients") return [] as T;
    throw new Error("Synthetic secret must not be rendered");
  };
  const handle = mountProvidersWorkspace(view, invoke, { writeText: async () => {} }); await handle.ready;
  assert.equal(view.querySelectorAll("[data-provider-card]").length, 3);
  assert.match(view.querySelector('[data-provider-card="claude"]')!.textContent!, /Manual browser capture/);
  assert.match(view.textContent!, /handshake.*not verified/i);
  assert.doesNotMatch(view.textContent!, /Synthetic secret/);
  assert.ok(view.querySelector<HTMLButtonElement>("[data-provider-write]")!.disabled);
  assert.ok(view.querySelector<HTMLSelectElement>("[data-provider-scope]")!.disabled);
  assert.ok(calls.every(cmd => /^(get_|list_)/.test(cmd)), "opening page does not start or grant anything");
  handle.destroy(); assert.equal(view.textContent, ""); dom.window.close();
});

test("late provider statuses cannot repopulate a destroyed page", async () => {
  const dom = new JSDOM("<main></main>"); const view = dom.window.document.querySelector("main")!;
  let finish!: (value: unknown) => void;
  const pending = new Promise(resolve => { finish = resolve; });
  const invoke: Invoke = async <T>(cmd: string) => (cmd === "list_local_mcp_clients" ? [] : await pending) as T;
  const handle = mountProvidersWorkspace(view, invoke, { writeText: async () => {} });
  handle.destroy(); finish({ running: true, connection_verified: false }); await delay(0);
  assert.equal(view.textContent, ""); await handle.ready; dom.window.close();
});

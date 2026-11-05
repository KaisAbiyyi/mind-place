import assert from "node:assert/strict";
import { test } from "node:test";
import { setTimeout as delay } from "node:timers/promises";
import { JSDOM } from "jsdom";
import { mountProvidersWorkspace } from "../src/providers-workspace.js";
import { mountReader, type Invoke } from "../src/reader.js";

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

test("provider Import a transcript opens the existing file-selection workflow", async () => {
  const dom = new JSDOM("<div id='app'></div>", { url: "https://fixture.test" });
  const root = dom.window.document.querySelector<HTMLElement>("#app")!;
  const invoke: Invoke = async <T>(cmd: string) => {
    if (cmd === "list_conversations" || cmd === "list_local_mcp_clients") return [] as T;
    if (cmd === "get_setup") return { archive_path: "fixture", extension_path: "fixture", host_path: "fixture", extension_id: "fixture", native_host_name: "fixture", registered_browsers: [], platform: "windows", legacy_available: false } as T;
    return { running: false, connection_verified: false } as T;
  };
  const handle = mountReader({ root, invoke }); await handle.ready;
  root.querySelector<HTMLButtonElement>('#knowledge-os-nav [data-route="ai-providers-mcp"]')!.click(); await delay(0);
  const button = [...root.querySelectorAll<HTMLButtonElement>(".providers-workspace button")].find(button => button.textContent === "Import a transcript")!;
  button.click(); await delay(0);
  assert.ok(root.querySelector("#setup-dialog")!.hasAttribute("open"));
  const input = root.querySelector<HTMLInputElement>("[data-generic-file]"); assert.ok(input);
  assert.equal(input.closest<HTMLElement>("[data-settings-panel]")!.hidden, false);
  assert.equal(root.ownerDocument.activeElement, input);
  handle.destroy(); dom.window.close();
});

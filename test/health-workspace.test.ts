import assert from "node:assert/strict";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import { mountHealthWorkspace } from "../src/health-workspace.js";
import type { Invoke } from "../src/reader.js";

const tick = async () => { await new Promise(resolve => setTimeout(resolve, 0)); };
function setup(invoke: Invoke) {
  const dom = new JSDOM("<!doctype html><main id='view'></main>");
  const view = dom.window.document.querySelector<HTMLElement>("#view")!;
  return { dom, view, handle: mountHealthWorkspace(view, invoke, () => {}) };
}
test("health: partial failure never fabricates connectivity or surfaces secrets", async () => {
  const seen: string[] = [];
  const invoke: Invoke = async <T>(name: string) => {
    seen.push(name);
    if (name === "get_security_status") return { locked: false, configured: true, epoch: 1 } as T;
    if (name === "get_archive_index_stats") return { sources: 1, revisions: 2, chunks: 3, generation: 4 } as T;
    if (name === "get_local_mcp_status") return { running: true, token: "SECRET" } as T;
    if (name === "get_hosted_tunnel_status") throw Error("runtime key SECRET");
    throw Error(name);
  };
  const { view, handle } = setup(invoke);
  await handle.refresh();
  assert.match(view.textContent!, /Partial status/);
  assert.match(view.textContent!, /Process running/);
  assert.match(view.textContent!, /Unavailable/);
  assert.doesNotMatch(view.textContent!, /SECRET/);
  assert.ok(seen.includes("get_archive_index_stats"));
  handle.destroy();
  assert.equal(view.textContent, "");
});
test("health: only metadata is rendered from an explicit inspection", async () => {
  const invoke: Invoke = async <T>(name: string) => {
    if (name === "get_security_status") return { locked: false, configured: true, epoch: 3 } as T;
    if (name === "get_archive_index_stats") return { sources: 0, revisions: 0, chunks: 0, generation: 0 } as T;
    if (name === "get_local_mcp_status" || name === "get_hosted_tunnel_status") return { running: false } as T;
    if (name === "inspect_retrieval") return {
      result: { hits: [{ snippet: "VERY PRIVATE", source_url: "https://secret.invalid" }] },
      trace: { operation: "search", elapsed_us: 501, index_generation: 2, result_count: 1, candidate_count: 2, source_count: 1, truncated: false },
    } as T;
    throw Error(name);
  };
  const { dom, view, handle } = setup(invoke);
  await handle.refresh();
  const input = view.querySelector<HTMLInputElement>("input")!;
  input.value = "alpha";
  view.querySelector("form")!.dispatchEvent(new dom.window.Event("submit", { bubbles: true, cancelable: true }));
  await tick(); await tick();
  assert.match(view.textContent!, /Candidates: 2/);
  assert.doesNotMatch(view.textContent!, /VERY PRIVATE|secret.invalid/);
  handle.destroy();
});
test("health: lock or destroy suppresses stale completions and clears sensitive UI", async () => {
  let finish!: (v: unknown) => void;
  const delayed = new Promise(resolve => { finish = resolve; });
  const invoke: Invoke = async <T>(name: string) => name === "get_security_status"
    ? { locked: false, configured: true, epoch: 2 } as T
    : name === "get_archive_index_stats" ? await delayed as T
      : { running: false } as T;
  const { view, handle } = setup(invoke);
  handle.destroy();
  finish({ generation: 3, sources: 4, revisions: 5, chunks: 6 });
  await tick();
  assert.equal(view.textContent, "");
});

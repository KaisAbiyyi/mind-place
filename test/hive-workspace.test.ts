import assert from "node:assert/strict";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import { layoutHive, mountHiveWorkspace, validateHive, type HiveGraph } from "../src/hive-workspace.js";
import type { Invoke } from "../src/reader.js";

const fixture: HiveGraph = {
  nodes: [
    { id: "a", kind: "conversation", label: "<img src=x onerror=evil()>", canonical_key: "a", source_ref: "srcA", depth: 0 },
    { id: "b", kind: "revision", label: "v2", canonical_key: "b", source_ref: "srcA", depth: 1 },
  ],
  edges: [
    { id: "edge1", from_node: "a", to_node: "b", relation: "version", origin: "inferred",
      confidence_bps: 9000, source_ref: "srcA", source_revision: "rev1", locator: "archive:a" },
    { id: "edge2", from_node: "a", to_node: "b", relation: "version", origin: "deterministic",
      confidence_bps: null, source_ref: "srcA", source_revision: "rev2", locator: "archive:b" },
  ],
  truncated: false,
};
const tick = async () => { await new Promise(resolve => setTimeout(resolve, 0)); };
function setup(invoke: Invoke) {
  const dom = new JSDOM("<!doctype html><main id='view'></main>");
  const view = dom.window.document.querySelector<HTMLElement>("#view")!;
  return { dom, view, handle: mountHiveWorkspace(view, invoke, () => {}, "source-seed") };
}
test("hive: rejects oversized, dangling and malformed graph data", () => {
  assert.throws(() => validateHive({ ...fixture, nodes: Array.from({ length: 121 }, (_, i) => ({ ...fixture.nodes[0], id: String(i) })) }));
  assert.throws(() => validateHive({ ...fixture, edges: [{ ...fixture.edges[0], to_node: "missing" }] }));
  assert.throws(() => validateHive({ ...fixture, nodes: [{ ...fixture.nodes[0], kind: "file" }] }));
  assert.ok(layoutHive(fixture.nodes).has("a"));
  assert.deepEqual(layoutHive(fixture.nodes), layoutHive([...fixture.nodes].reverse()));
});
test("hive: navigation, bounded SVG, evidence revision controls and inert labels", async () => {
  const calls: string[] = [];
  const invoke: Invoke = async <T>(name: string) => {
    calls.push(name);
    if (name === "get_security_status") return { locked: false, epoch: 9 } as T;
    if (name === "read_archive_hive") return fixture as T;
    throw Error(name);
  };
  const { dom, view, handle } = setup(invoke);
  view.querySelector("form")!.dispatchEvent(new dom.window.Event("submit", { bubbles: true, cancelable: true }));
  await tick(); await tick();
  assert.match(view.textContent!, /2 nodes · 2 edges/);
  assert.equal(view.querySelectorAll("svg circle").length, 2);
  assert.equal(view.querySelectorAll("svg line").length, 2);
  assert.equal(view.querySelector("img"), null);
  assert.ok(calls.includes("read_archive_hive"));
  const ranges = view.querySelectorAll<HTMLInputElement>('input[type="range"]');
  ranges[1].value = "1";
  ranges[1].dispatchEvent(new dom.window.Event("input", { bubbles: true }));
  assert.equal(view.querySelectorAll("svg line").length, 1);
  view.querySelectorAll<HTMLButtonElement>(".hive-node-choice")[1].click();
  assert.match(view.querySelector(".hive-details")!.textContent!, /revision/);
  handle.destroy();
  assert.equal(view.textContent, "");
});
test("hive: lock boundary and destroy suppress late data", async () => {
  let finish!: (v: unknown) => void;
  const pending = new Promise(resolve => { finish = resolve; });
  const invoke: Invoke = async <T>(name: string) =>
    name === "get_security_status" ? { locked: false, epoch: 1 } as T : await pending as T;
  const { dom, view, handle } = setup(invoke);
  view.querySelector("form")!.dispatchEvent(new dom.window.Event("submit", { bubbles: true, cancelable: true }));
  await tick();
  handle.destroy();
  finish(fixture);
  await tick();
  assert.equal(view.textContent, "");
});

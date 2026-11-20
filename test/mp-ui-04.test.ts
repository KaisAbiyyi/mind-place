import assert from "node:assert/strict";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import { layoutHive, mountHiveWorkspace, type HiveGraph } from "../src/hive-workspace.js";
import type { Invoke } from "../src/reader.js";

const graph: HiveGraph = {
  nodes: [
    { id: "a", kind: "import", label: "Project archive", canonical_key: "archive-123", source_ref: "src1", depth: 0 },
    { id: "b", kind: "conversation", label: "Follow-up", canonical_key: "internal", source_ref: "src1", depth: 1 },
    { id: "c", kind: "revision", label: "Isolated revision", canonical_key: "rev", source_ref: "src2", depth: 0 },
  ],
  edges: [
    { id: "ab", from_node: "a", to_node: "b", relation: "revision", origin: "inferred", confidence_bps: 8100, source_ref: "src1", source_revision: "v2", locator: "line:5" },
  ],
  truncated: false,
};
const tick = async () => { await new Promise(resolve => setTimeout(resolve, 0)); };

test("UI-04 topology is independent of source order and never joins isolated memories", () => {
  const forward = layoutHive(graph.nodes, graph.edges);
  const reversed = layoutHive([...graph.nodes].reverse(), [...graph.edges].reverse());
  assert.deepEqual(forward, reversed);
  const dist = (a: string, b: string) => Math.hypot(forward.get(a)!.x-forward.get(b)!.x, forward.get(a)!.y-forward.get(b)!.y);
  assert.ok(dist("a", "b") < dist("a", "c"));
});

test("UI-04 search, filter, evidence, branch history and exact-source navigation", async () => {
  const dom = new JSDOM("<!doctype html><div id='v'></div>");
  const view = dom.window.document.querySelector<HTMLElement>("#v")!;
  const opened: string[] = [];
  const invoke: Invoke = async <T>(name: string) => {
    if (name === "get_security_status") return { locked: false, epoch: 42 } as T;
    if (name === "read_archive_hive") return graph as T;
    throw Error(name);
  };
  const handle = mountHiveWorkspace(view, invoke, () => {}, "archive-123", id => opened.push(id));
  try {
    view.querySelector("form")!.dispatchEvent(new dom.window.Event("submit", { bubbles: true, cancelable: true }));
    await tick(); await tick();
    assert.equal(view.querySelectorAll("svg circle").length, 3);
    assert.equal(view.querySelectorAll(".hive-isolated").length, 1);
    const selections = [...view.querySelectorAll<HTMLButtonElement>(".hive-node-choice")];
    selections.find(b => b.textContent!.includes("Project archive"))!.click();
    const open = [...view.querySelectorAll<HTMLButtonElement>(".hive-details button")]
      .find(b => b.textContent!.includes("matching saved conversation"))!;
    open.click(); assert.deepEqual(opened, ["archive-123"]);
    const follow = [...view.querySelectorAll<HTMLButtonElement>(".hive-details button")]
      .find(b => b.textContent!.includes("Follow branch"))!;
    follow.click();
    assert.match(view.querySelector(".hive-details")!.textContent!, /Follow-up/);
    const back = [...view.querySelectorAll<HTMLButtonElement>(".hive-controls button")]
      .find(b => b.textContent === "Back one mind")!;
    back.click(); assert.match(view.querySelector(".hive-details h2")!.textContent!, /Project archive/);
    const search = view.querySelector<HTMLInputElement>('.hive-controls input[type="search"]')!;
    search.value = "isolated"; search.dispatchEvent(new dom.window.Event("input"));
    assert.equal(view.querySelectorAll("svg circle").length, 1);
    assert.equal(view.querySelectorAll("svg line").length, 0);
    search.value = ""; search.dispatchEvent(new dom.window.Event("input"));
    const layer = [...view.querySelectorAll<HTMLSelectElement>(".hive-controls select")][0];
    layer.value = "revision"; layer.dispatchEvent(new dom.window.Event("change"));
    assert.equal(view.querySelectorAll("svg circle").length, 1);
    const reset = [...view.querySelectorAll<HTMLButtonElement>(".hive-controls button")]
      .find(b => b.textContent === "Reset view")!;
    reset.click();
    assert.match(view.querySelector(".hive-details")!.textContent!, /Select a node|No nodes match|Project archive/);
  } finally { handle.destroy(); dom.window.close(); }
});

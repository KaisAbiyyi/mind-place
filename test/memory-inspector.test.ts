import assert from "node:assert/strict";
import { test } from "node:test";
import { setTimeout as delay } from "node:timers/promises";
import { JSDOM } from "jsdom";
import { mountMemoryInspector, type RankedMemory, type MemoryDetail } from "../src/memory-inspector.js";
import type { Invoke } from "../src/reader.js";

function fixture(invoke: Invoke, copied: string[] = []) {
  const dom = new JSDOM("<!doctype html><div id='reader'></div>");
  const view = dom.window.document.getElementById("reader")!;
  const handle = mountMemoryInspector({
    view, invoke, copyText: async (text) => { copied.push(text); }, onClose: () => handle.destroy(),
  });
  return { view, handle, copied };
}
function candidate(id: string, statement: string, disposition = "contested"): RankedMemory {
  return {
    claim: { id, statement, asserted_by: "user", epistemic_origin: "explicit", status: "active",
      revision: 1, recorded_at_ms: 1792760000000, evidence: [] },
    disposition, score: 100, related_claim_ids: [],
  };
}
function detail(item: RankedMemory): MemoryDetail {
  return {
    ranked: item,
    history: [{ revision: 1, recorded_at_ms: 1792760000000, event_type: "created", status: "active",
      reason: null, superseded_by: null, evidence: [{
        source_ref: "src:v1:opaque", evidence_kind: "support",
      }] }],
    relations: [{ other_claim_id: "claim:other", direction: "incoming", relation: "contradicts",
      source_ref: "src:v1:opaque", reason: "<img src=x onerror=alert(1)>",
      recorded_at_ms: 1792760000000, resolved_at_ms: null }],
  };
}

test("contested and historical claims are visible without rendering provenance as HTML", async () => {
  const item = candidate("claim:old", "<script>inert statement</script>", "contested");
  const invoke: Invoke = async <T>(command) => {
    if (command === "list_memory_inspection") return [item] as T;
    if (command === "get_memory_inspection") return detail(item) as T;
    throw new Error(command);
  };
  const { view, handle } = fixture(invoke);
  await delay(0);
  assert.match(view.textContent ?? "", /contested/);
  view.querySelector<HTMLButtonElement>("[data-memory-claim]")!.click();
  await delay(0);
  assert.match(view.textContent ?? "", /must not be asserted as fact/);
  assert.match(view.textContent ?? "", /<img src=x onerror=alert\(1\)>/);
  assert.equal(view.querySelector("script, img, iframe"), null);
  assert.match(view.textContent ?? "", /src:v1:opaque/);
  handle.destroy();
});

test("manual correction copies a draft only, pin is view-only", async () => {
  const item = candidate("claim:a", "Uncertain preference");
  const invoke: Invoke = async <T>(command) => {
    if (command === "list_memory_inspection") return [item] as T;
    if (command === "get_memory_inspection") return detail(item) as T;
    throw new Error("Unexpected mutation: " + command);
  };
  const { view, handle, copied } = fixture(invoke);
  await delay(0);
  view.querySelector<HTMLButtonElement>("[data-memory-claim]")!.click();
  await delay(0);
  view.querySelector<HTMLButtonElement>("[data-memory-action='pin']")!.click();
  assert.match(view.textContent ?? "", /Pinned in this view/);
  const input = view.querySelector<HTMLTextAreaElement>("#memory-correction")!;
  input.value = "Updated, source-backed preference";
  view.querySelector<HTMLButtonElement>("[data-memory-action='copy-correction']")!.click();
  await delay(0);
  assert.equal(copied.length, 1);
  assert.match(copied[0], /not applied/);
  assert.match(view.textContent ?? "", /memory unchanged/);
  handle.destroy();
});

test("forget lookup fails closed and stale asynchronous details never reappear after destroy", async () => {
  const item = candidate("claim:gone", "Old detail");
  let finish!: (value: MemoryDetail | null) => void;
  const pending = new Promise<MemoryDetail | null>((resolve) => { finish = resolve; });
  const invoke: Invoke = async <T>(command) => {
    if (command === "list_memory_inspection") return [item] as T;
    if (command === "get_memory_inspection") return pending as Promise<T>;
    throw new Error(command);
  };
  const { view, handle } = fixture(invoke);
  await delay(0);
  view.querySelector<HTMLButtonElement>("[data-memory-claim]")!.click();
  handle.destroy();
  finish(detail(item));
  await delay(0);
  assert.equal(view.textContent, "");
});

test("forgotten/no-visible result never displays stale memory details", async () => {
  const item = candidate("claim:gone", "Disallowed detail");
  const invoke: Invoke = async <T>(command) => {
    if (command === "list_memory_inspection") return [item] as T;
    if (command === "get_memory_inspection") return null as T;
    throw new Error(command);
  };
  const { view, handle } = fixture(invoke);
  await delay(0);
  view.querySelector<HTMLButtonElement>("[data-memory-claim]")!.click();
  await delay(0);
  assert.match(view.textContent ?? "", /unavailable or forgotten/i);
  handle.destroy();
});

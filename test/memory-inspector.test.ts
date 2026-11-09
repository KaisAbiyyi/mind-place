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
  const invoke: Invoke = async <T>(command: string) => {
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
  const invoke: Invoke = async <T>(command: string) => {
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
  const invoke: Invoke = async <T>(command: string) => {
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
  const invoke: Invoke = async <T>(command: string) => {
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

test("memory collections filter visible dispositions without inventing persisted taxonomy", async () => {
  const entries = [candidate("a", "Current hypothesis", "current_candidate"), candidate("b", "Disputed hypothesis", "contested")];
  const invoke: Invoke = async <T>() => entries as T;
  const { view, handle } = fixture(invoke); await delay(0);
  const collection = view.querySelector<HTMLButtonElement>('[data-memory-collection="contested"]');
  assert.ok(collection, "visible disposition collection is reachable"); collection.focus(); collection.click();
  assert.equal(view.ownerDocument.activeElement?.getAttribute("data-memory-collection"), "contested");
  assert.equal(view.querySelectorAll("[data-memory-claim]").length, 1);
  assert.match(view.querySelector(".memory-cards")!.textContent!, /Disputed hypothesis/);
  assert.match(view.textContent!, /view collections/i);
  handle.destroy();
});

test("memory quick drawer restores focus and history selects the authorized revision", async () => {
  const item = candidate("a", "Current statement"); const data = detail(item);
  data.history.push({ ...data.history[0], revision: 2, recorded_at_ms: 1792760010000, status: "challenged", reason: "New evidence" });
  const invoke: Invoke = async <T>(cmd: string) => (cmd === "list_memory_inspection" ? [item] : data) as T;
  const { view, handle } = fixture(invoke); await delay(0);
  const quick = view.querySelector<HTMLButtonElement>('[data-memory-quick="a"]'); assert.ok(quick); quick.focus(); quick.click(); await delay(0);
  const drawer = view.querySelector<HTMLDialogElement>(".memory-quick-drawer"); assert.ok(drawer); assert.ok(drawer.hasAttribute("open"));
  drawer.dispatchEvent(new view.ownerDocument.defaultView!.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  assert.equal(view.ownerDocument.activeElement, quick);
  view.querySelector<HTMLButtonElement>("[data-memory-claim]")!.click(); await delay(0);
  const revision = view.querySelector<HTMLSelectElement>("[data-memory-as-of]"); assert.ok(revision);
  revision.value = "2"; revision.dispatchEvent(new view.ownerDocument.defaultView!.Event("change"));
  assert.match(view.querySelector("[data-memory-historical]")!.textContent!, /challenged/);
  assert.ok(view.querySelector<HTMLButtonElement>('[data-memory-write="forget"]')!.disabled);
  handle.destroy();
});

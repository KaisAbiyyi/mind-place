import assert from "node:assert/strict";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import {
  WORKSPACES, createWorkspaceNavigation, setActiveWorkspace, getVirtualWindow,
  createKnowledgeCollection, VIRTUAL_LIST_THRESHOLD,
} from "../src/knowledge-os.js";
import { mountReader, type Invoke } from "../src/reader.js";
import type { Conversation, ConversationSummary } from "../src/contracts.js";

function sample(id: number): ConversationSummary {
  const key = id.toString(16).padStart(64, "0");
  return {
    id: key, conversation_id: key, provider: "chatgpt",
    title: "Conversation " + id, source_url: "https://chatgpt.com/c/" + key,
    captured_at: "2026-10-20T00:00:00Z", message_count: 1, snapshot_count: 1,
    completeness: "complete", warnings: [],
  };
}

test("workspace registry is explicit, accessible and does not imply unfinished panels exist", () => {
  const doc = new JSDOM("<!doctype html><html><body></body></html>").window.document;
  const nav = createWorkspaceNavigation(doc);
  assert.equal(nav.getAttribute("aria-label"), "Knowledge workspaces");
  assert.equal(nav.querySelectorAll("button").length, 4);
  assert.equal(WORKSPACES.filter(x => x.phase === "planned").length, 5);
  assert.equal(nav.querySelector("button[disabled]"), null);
  assert.equal(nav.querySelector('[data-workspace="graph"]'), null);
  assert.equal(nav.querySelectorAll('[aria-current="page"]').length, 1);
  setActiveWorkspace(nav, "memories");
  assert.equal(nav.querySelector('[data-workspace="memories"]')?.getAttribute("aria-current"), "page");
  assert.equal(nav.querySelectorAll('[aria-current="page"]').length, 1);
});

test("virtual viewport bounds 100k items to a small constant number of rows", () => {
  const visible = getVirtualWindow(100_000, 76_000, 304);
  assert.ok(visible.start <= 1_000 && visible.end > 1_000);
  assert.ok(visible.end - visible.start <= 18);
  assert.equal(visible.beforePx, visible.start * 76);
  assert.equal(visible.afterPx, (100_000 - visible.end) * 76);
  assert.deepEqual(getVirtualWindow(0, 0, 0), { start: 0, end: 0, beforePx: 0, afterPx: 0 });
  const ending = getVirtualWindow(11, Infinity, 320);
  assert.equal(ending.start, 0);
  assert.equal(ending.end, 11);
  assert.ok(getVirtualWindow(1000, -500, NaN).start === 0);
});

test("cursor collection deduplicates concurrent loads and ignores stale results on reset", async () => {
  let calls = 0;
  let finish!: (value: { items: number[]; nextCursor: string | null }) => void;
  const first = new Promise<{ items: number[]; nextCursor: string | null }>(resolve => { finish = resolve; });
  const resource = createKnowledgeCollection(async ({ cursor, limit }) => {
    calls++;
    assert.equal(limit, 25);
    if (!cursor) return first;
    assert.equal(cursor, "page-2");
    return { items: [3], nextCursor: null };
  }, 25);
  const a = resource.loadMore();
  const b = resource.loadMore();
  assert.equal(calls, 1);
  assert.equal(resource.snapshot().status, "loading");
  finish({ items: [1, 2], nextCursor: "page-2" });
  await Promise.all([a, b]);
  assert.deepEqual(resource.snapshot().items, [1, 2]);
  await resource.loadMore();
  assert.deepEqual(resource.snapshot().items, [1, 2, 3]);
  assert.equal(resource.snapshot().nextCursor, null);
  await resource.loadMore();
  assert.equal(calls, 2);
  resource.reset();
  assert.deepEqual(resource.snapshot().items, []);
  assert.equal(resource.snapshot().status, "idle");

  let stale!: (value: { items: number[]; nextCursor: string | null }) => void;
  const delayed = createKnowledgeCollection(() => new Promise(resolve => { stale = resolve; }));
  const request = delayed.loadMore();
  delayed.reset();
  stale({ items: [999], nextCursor: null });
  await request;
  assert.deepEqual(delayed.snapshot().items, []);
});

test("cursor collection rejects loops, keeps previous page on retry and caps page size", async () => {
  let calls = 0;
  const resource = createKnowledgeCollection(async ({ limit }) => {
    assert.equal(limit, 200);
    calls++;
    return calls === 1 ? { items: [1], nextCursor: "again" }
      : calls === 2 ? { items: [2], nextCursor: "again" }
        : { items: [2], nextCursor: null };
  }, 50_000);
  await resource.loadMore();
  await resource.loadMore();
  assert.equal(resource.snapshot().status, "error");
  assert.deepEqual(resource.snapshot().items, [1]);
  await resource.loadMore();
  assert.equal(resource.snapshot().status, "ready");
  assert.deepEqual(resource.snapshot().items, [1, 2]);
});

test("reader virtualizes large archives and navigates visible rows without loading unmounted DOM", async () => {
  const dom = new JSDOM("<!doctype html><html><body><div id='app'></div></body></html>", { url: "https://archive.test/" });
  const root = dom.window.document.getElementById("app")!;
  const entries = Array.from({ length: 500 }, (_, i) => sample(i));
  assert.ok(entries.length > VIRTUAL_LIST_THRESHOLD);
  let reading = 0;
  const invoke: Invoke = async <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
    if (command === "list_conversations") return entries as T;
    if (command === "list_snapshots") return [] as T;
    if (command === "read_conversation") {
      reading++;
      const item = entries.find(e => e.id === args?.id)!;
      return {
        summary: item,
        capture: {
          schema_version: 1, provider: "chatgpt", conversation_id: item.conversation_id,
          source_url: item.source_url, title: item.title, captured_at: item.captured_at,
          capture_method: "manual-dom", completeness: "complete", warnings: [],
          messages: [],
        },
      } satisfies Conversation as T;
    }
    throw new Error("unexpected: " + command);
  };
  const handle = mountReader({ root, invoke });
  try {
    await handle.ready;
    await Promise.resolve();
    const list = root.querySelector<HTMLElement>("#conversation-list")!;
    assert.ok(list.classList.contains("is-virtualized"));
    assert.ok(list.querySelectorAll(".conversation-row").length < 40);
    assert.equal(root.querySelectorAll('[aria-current="page"]').length, 1);
    list.scrollTop = 7_600;
    list.dispatchEvent(new dom.window.Event("scroll"));
    assert.ok(list.querySelector('[data-row-index="100"]'));
    assert.equal(list.querySelector('[data-row-index="0"]'), null);
    const row100 = list.querySelector<HTMLButtonElement>('[data-row-index="100"]')!;
    row100.focus();
    row100.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
    assert.equal(dom.window.document.activeElement?.getAttribute("data-row-index"), "101");
    assert.ok(reading > 0);
  } finally {
    handle.destroy();
  }
  assert.equal(root.childElementCount, 0);
});

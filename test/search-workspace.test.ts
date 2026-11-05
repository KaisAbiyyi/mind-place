import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import { mountSearchWorkspace, type ContextPack, type RetrievalResponse, type SearchInspection } from "../src/search-workspace.js";
import type { Invoke, ClipboardWriter } from "../src/reader.js";

function createFixture(invoke: Invoke, clipboard?: ClipboardWriter) {
  const dom = new JSDOM("<!doctype html><html><body><main id='view'></main></body></html>", { url: "https://local.test/" });
  const view = dom.window.document.querySelector<HTMLElement>("#view")!;
  const handle = mountSearchWorkspace(view, invoke, {
    clipboard,
    onClose: () => handle.destroy(),
  });
  return { dom, view, handle };
}

const mockArchiveHit = {
  source_id: "s1",
  archive_id: "arch1",
  snapshot_id: "snap1",
  message_id: "msg1",
  kind: "body",
  role: "assistant",
  title: "AI Conversation",
  snippet: "Found relevant evidence",
  score: 1.25,
};

const mockDocHit = {
  uri: "mpdoc://approved/manual.pdf",
  version_id: "v1",
  section: "intro",
  snippet: "Document snippet text",
  score: 0.95,
  extractor_revision: "pdf-v1",
  warnings: [],
};

const mockRetrievalResponse: RetrievalResponse = {
  hits: [
    {
      source_id: "s1",
      archive_id: "arch1",
      snapshot_id: "snap1",
      conversation_id: "conv1",
      provider: "chatgpt",
      source_url: "https://chatgpt.com/c/conv1",
      captured_at: "2026-10-24T12:00:00Z",
      completeness: "complete",
      message_id: "msg1",
      part: 0,
      kind: "body",
      role: "assistant",
      title: "AI Conversation",
      snippet: "Weighted match chunk",
      score: 2.45,
      matched_queries: [0],
      exact_queries: [0],
      score_contributions: [
        { query_index: 0, weight: 1.0, bm25: -2.15, lexical_relevance: 1.45, exact_bonus: 1.0, weighted_score: 2.45 },
      ],
    },
  ],
  candidate_count: 5,
  truncated: false,
};

const mockContextPack: ContextPack = {
  groups: [
    {
      source_id: "chatgpt",
      archive_id: "arch1",
      snapshot_id: "snap1",
      conversation_id: "conv1",
      title: "AI Conversation",
      source_url: "https://chatgpt.com/c/conv1",
      captured_at: "2026-10-24T12:00:00Z",
      completeness: "complete",
      indexed_is_current: true,
      excerpts: [
        {
          kind: "body",
          message_id: "msg1",
          role: "assistant",
          text: "Verified citation excerpt text",
          byte_start: 0,
          byte_end: 30,
          total_bytes: 120,
          score: 2.45,
          matched_queries: [0],
        },
      ],
    },
  ],
  total_tokens: 350,
  truncated: false,
};

const mockSearchInspection: SearchInspection = {
  trace: {
    operation: "retrieve",
    elapsed_us: 420,
    index_generation: 7,
    candidate_count: 12,
    result_count: 1,
    source_count: 1,
    estimated_tokens: 350,
    truncated: false,
  },
  result: mockRetrievalResponse,
};

test("MP-UI-07: multi-query search, filters and pagination support beyond 12 results", async () => {
  const commands: string[] = [];
  const invoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
    commands.push(command);
    if (command === "get_security_status") return { configured: true, locked: false, epoch: 1 } as T;
    if (command === "search_archive_index") {
      // Return 25 results to verify pagination
      const hits = Array.from({ length: 25 }, (_, i) => ({
        ...mockArchiveHit,
        archive_id: `arch_${i}`,
        title: `Chat ${i + 1}`,
      }));
      return hits as T;
    }
    if (command === "search_document_index") return [mockDocHit] as T;
    if (command === "search_retrieval") return mockRetrievalResponse as T;
    throw new Error("Unhandled: " + command);
  };

  const { dom, view, handle } = createFixture(invoke);

  // Toggle multi-query mode
  const toggleBtn = view.querySelector<HTMLButtonElement>("[data-action='toggle-multi-query']")!;
  toggleBtn.click();
  assert.equal(toggleBtn.getAttribute("aria-expanded"), "true");
  assert.ok(view.querySelector(".search-term-row"));

  // Add a second query term
  view.querySelector<HTMLButtonElement>("[data-action='add-query-term']")!.click();
  const termRows = view.querySelectorAll(".search-term-row");
  assert.equal(termRows.length, 2);

  // Submit search
  view.querySelector<HTMLInputElement>("#library-query")!.value = "architecture";
  view.querySelector("form")!.dispatchEvent(new dom.window.Event("submit", { bubbles: true, cancelable: true }));
  await delay(10);

  assert.ok(commands.includes("search_archive_index"));
  assert.ok(commands.includes("search_document_index"));
  assert.ok(commands.includes("search_retrieval"));

  // Verify pagination renders (25 archive + 1 doc + 1 retrieval = 27 items, page size 10 -> 3 pages)
  const paginationNav = view.querySelector<HTMLElement>(".search-pagination-nav")!;
  assert.equal(paginationNav.hidden, false);
  const indicator = view.querySelector<HTMLElement>(".search-page-indicator")!;
  assert.match(indicator.textContent!, /Page 1 of 3/);

  // Navigate to Page 2
  view.querySelector<HTMLButtonElement>("[data-action='next-page']")!.click();
  assert.match(indicator.textContent!, /Page 2 of 3/);

  // Navigate to Page 1
  view.querySelector<HTMLButtonElement>("[data-action='prev-page']")!.click();
  assert.match(indicator.textContent!, /Page 1 of 3/);

  handle.destroy();
});

test("MP-UI-07: Context Pack assembly and formatted clipboard export", async () => {
  let copiedText = "";
  const clipboard: ClipboardWriter = {
    writeText: async (text: string) => {
      copiedText = text;
    },
  };

  const invoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
    if (command === "get_security_status") return { configured: true, locked: false, epoch: 1 } as T;
    if (command === "search_archive_index") return [mockArchiveHit] as T;
    if (command === "search_document_index") return [] as T;
    if (command === "build_retrieval_context") {
      assert.ok(args?.request);
      return mockContextPack as T;
    }
    throw new Error("Unhandled: " + command);
  };

  const { dom, view, handle } = createFixture(invoke, clipboard);

  view.querySelector<HTMLInputElement>("#library-query")!.value = "quantum";
  view.querySelector("form")!.dispatchEvent(new dom.window.Event("submit", { bubbles: true, cancelable: true }));
  await delay(10);

  const buildBtn = view.querySelector<HTMLButtonElement>("[data-action='build-context']")!;
  assert.equal(buildBtn.disabled, false);
  buildBtn.click();
  await delay(10);

  const contextDrawer = view.querySelector<HTMLDialogElement>("#search-context-drawer")!;
  assert.ok(contextDrawer.hasAttribute("open"));
  assert.match(view.querySelector(".search-context-pack-content")!.textContent!, /Verified citation excerpt text/);

  // Copy context pack
  view.querySelector<HTMLButtonElement>("[data-action='copy-context']")!.click();
  await delay(10);
  assert.ok(copiedText.includes("Verified citation excerpt text"));
  assert.match(view.querySelector(".search-status-text")!.textContent!, /copied to clipboard/);

  handle.destroy();
});

test("MP-UI-07: Technical trace inspector displays latency and BM25 score contributions", async () => {
  const invoke: Invoke = async <T>(command: string) => {
    if (command === "get_security_status") return { configured: true, locked: false, epoch: 1 } as T;
    if (command === "search_archive_index") return [mockArchiveHit] as T;
    if (command === "search_document_index") return [] as T;
    if (command === "inspect_retrieval") return mockSearchInspection as T;
    throw new Error("Unhandled: " + command);
  };

  const { dom, view, handle } = createFixture(invoke);

  view.querySelector<HTMLInputElement>("#library-query")!.value = "diagnostics";
  view.querySelector("form")!.dispatchEvent(new dom.window.Event("submit", { bubbles: true, cancelable: true }));
  await delay(10);

  const inspectBtn = view.querySelector<HTMLButtonElement>("[data-action='inspect-trace']")!;
  assert.equal(inspectBtn.disabled, false);
  inspectBtn.click();
  await delay(10);

  const traceDrawer = view.querySelector<HTMLDialogElement>("#search-trace-drawer")!;
  assert.ok(traceDrawer.hasAttribute("open"));

  const traceText = view.querySelector(".search-trace-content")!.textContent!;
  assert.match(traceText, /420 µs/);
  assert.match(traceText, /Index Generation: 7/);
  assert.match(traceText, /Candidates evaluated: 12/);
  assert.match(traceText, /-2.15/); // BM25 component

  handle.destroy();
});

test("MP-UI-07: Fail-closed on locked security status", async () => {
  const invoke: Invoke = async <T>(command: string) => {
    if (command === "get_security_status") return { configured: true, locked: true, epoch: 0 } as T;
    throw new Error("Should not be called when locked: " + command);
  };

  const { dom, view, handle } = createFixture(invoke);

  view.querySelector<HTMLInputElement>("#library-query")!.value = "classified";
  view.querySelector("form")!.dispatchEvent(new dom.window.Event("submit", { bubbles: true, cancelable: true }));
  await delay(10);

  assert.equal(handle.state().status, "locked");
  assert.match(view.textContent!, /Mind Place Locked/);

  handle.destroy();
  assert.equal(view.textContent, "");
});

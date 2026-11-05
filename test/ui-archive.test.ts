import assert from "node:assert/strict";
import { test } from "node:test";
import { setTimeout as delay } from "node:timers/promises";
import { JSDOM } from "jsdom";
import { mountReader, type Invoke } from "../src/reader.js";
import type { Conversation, ConversationSummary } from "../src/contracts.js";

function summary(id: string, provider = "chatgpt"): ConversationSummary {
  return { id, provider, conversation_id: id, title: "Saved " + id, source_url: "https://chatgpt.com/c/" + id,
    captured_at: "2026-11-01T00:00:00Z", message_count: 95, snapshot_count: 2, completeness: "partial", warnings: [] };
}
function conversation(item: ConversationSummary): Conversation {
  return { summary: item, snapshot_id: "snapshot-exact", capture: { schema_version: 1, provider: "chatgpt",
    conversation_id: item.id, title: item.title, source_url: item.source_url, captured_at: item.captured_at,
    capture_method: "manual-dom", completeness: "partial", warnings: [], messages: Array.from({ length: 95 }, (_, i) => ({
      id: "message-" + i, role: "assistant" as const, text: "Answer " + i, format: "plain" as const, attachments: [],
    })) } };
}

test("archive table filters actual provider metadata and pages bounded rows", async () => {
  const dom = new JSDOM("<div id='app'></div>", { url: "https://fixture.test" });
  const root = dom.window.document.querySelector<HTMLElement>("#app")!;
  const items = Array.from({ length: 125 }, (_, i) => summary(String(i), i % 2 ? "claude" : "chatgpt"));
  const invoke: Invoke = async <T>(cmd: string) => {
    if (cmd === "list_conversations") return items as T;
    if (cmd === "list_snapshots") return [] as T;
    if (cmd === "read_conversation") return conversation(items[0]) as T;
    throw new Error(cmd);
  };
  const reader = mountReader({ root, invoke });
  await reader.ready;
  root.querySelector<HTMLButtonElement>('[data-route="conversations"]')!.click();
  await delay(0);
  assert.equal(root.querySelector("#reader-view h1")?.textContent, "Conversations Archive");
  assert.equal(root.querySelectorAll("[data-archive-row]").length, 50);
  const provider = root.querySelector<HTMLSelectElement>("#conversation-provider-filter")!;
  provider.value = "claude"; provider.dispatchEvent(new dom.window.Event("change"));
  assert.equal(root.querySelectorAll("[data-archive-row]").length, 50);
  assert.match(root.querySelector("[data-archive-range]")!.textContent!, /62/);
  root.querySelector<HTMLButtonElement>('[data-archive-page="next"]')!.click();
  assert.equal(root.querySelectorAll("[data-archive-row]").length, 12);
  reader.destroy(); dom.window.close();
});

test("transcript pages reach every message and preserve exact snapshot provenance", async () => {
  const dom = new JSDOM("<div id='app'></div>", { url: "https://fixture.test" });
  const root = dom.window.document.querySelector<HTMLElement>("#app")!;
  const item = summary("one");
  const invoke: Invoke = async <T>(cmd: string) => {
    if (cmd === "list_conversations") return [item] as T;
    if (cmd === "read_conversation") return conversation(item) as T;
    if (cmd === "list_snapshots") return [] as T;
    throw new Error(cmd);
  };
  const reader = mountReader({ root, invoke }); await reader.ready;
  root.querySelector<HTMLButtonElement>("[data-conversation-id='one']")!.click(); await delay(0);
  assert.equal(root.querySelectorAll(".message").length, 40);
  assert.match(root.querySelector(".conversation-provenance")!.textContent!, /snapshot-exact/);
  root.querySelector<HTMLButtonElement>('[data-action="transcript-next"]')!.click();
  assert.equal(root.querySelector(".message")?.getAttribute("data-message-id"), "message-40");
  root.querySelector<HTMLButtonElement>('[data-action="transcript-next"]')!.click();
  assert.equal(root.querySelectorAll(".message").length, 15);
  assert.match(root.textContent!, /Answer 94/);
  root.querySelector<HTMLButtonElement>('[data-action="archive-table"]')!.click();
  assert.equal(root.querySelector("#reader-view h1")?.textContent, "Conversations Archive");
  reader.destroy(); assert.equal(root.textContent, ""); dom.window.close();
});

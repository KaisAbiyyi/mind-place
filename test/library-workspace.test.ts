import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import { mountLibraryWorkspace } from "../src/library-workspace.js";
import type { Invoke } from "../src/reader.js";

function fixture(invoke: Invoke) {
  const dom = new JSDOM("<!doctype html><html><body><main></main></body></html>", { url: "https://local.test/" });
  const view = dom.window.document.querySelector("main")!;
  const handle = mountLibraryWorkspace(view, invoke, () => handle.destroy());
  const search = (q: string) => {
    view.querySelector<HTMLInputElement>("#library-query")!.value = q;
    view.querySelector("form")!.dispatchEvent(new dom.window.Event("submit", { bubbles: true, cancelable: true }));
  };
  return { dom, view, handle, search };
}
const hit = { source_id:"s", archive_id:"archive", snapshot_id:"snap", message_id:"msg", kind:"body", role:"assistant", title:"Test", snippet:"<img src=x onerror=alert(1)>", score:1 };
test("search is bounded, excerpts inert, preview pinned to snapshot and source identity", async () => {
  const commands: string[] = [];
  const invoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
    commands.push(command);
    if (command === "search_archive_index") {
      assert.equal(args?.limit, 40);
      return [hit] as T;
    }
    if (command === "search_document_index") return [{ uri:"mpdoc://approved/a",version_id:"v1",section:"s",snippet:"note",score:1,extractor_revision:"r1",warnings:[] }] as T;
    if (command === "read_conversation") {
      assert.deepEqual(args, { id:"archive", snapshotId:"snap" });
      return { summary: {id:"archive",title:"Test",provider:"chatgpt"}, snapshot_id:"snap", capture: { captured_at:"2026-10-20", messages:[{id:"msg",text:"hello",role:"assistant"}] } } as T;
    }
    throw Error(command);
  };
  const {view,handle,search} = fixture(invoke);
  search("hello"); await delay(0);
  assert.equal(view.querySelectorAll(".knowledge-library-hit").length, 2);
  assert.equal(view.querySelector("img"), null);
  view.querySelector<HTMLButtonElement>("[data-library-archive]")!.click(); await delay(0);
  assert.match(view.querySelector(".knowledge-library-preview")!.textContent!, /Snapshot: snap/);
  assert.ok(commands.includes("read_conversation"));
  handle.destroy();
});
test("stale response after destroy cannot reveal source details", async () => {
  let resolve!: (value: any) => void;
  const pending = new Promise<any>(r=>resolve=r);
  const invoke: Invoke = async <T>(command: string) => command === "search_archive_index" ? pending as T : [] as T;
  const {view,handle,search} = fixture(invoke);
  search("secret"); handle.destroy(); resolve([hit]); await delay(0);
  assert.equal(view.textContent, "");
});
test("partial source failure is explicit rather than falsely claiming complete results", async () => {
  const invoke: Invoke = async <T>(command: string) => {
    if (command === "search_archive_index") return [hit] as T;
    throw Error("offline");
  };
  const {view,handle,search} = fixture(invoke);
  search("topic"); await delay(0);
  assert.match(view.textContent!, /Results may be incomplete/);
  assert.equal(view.querySelectorAll("[data-library-archive]").length, 1);
  handle.destroy();
});

test("title index hit previews exact snapshot title rather than a pseudo-message", async () => {
  const titleHit = { ...hit, kind:"title", role:"title", message_id:"@title", title:"Current heading" };
  const invoke: Invoke = async <T>(command: string) => {
    if (command === "search_archive_index") return [titleHit] as T;
    if (command === "search_document_index") return [] as T;
    if (command === "read_conversation") return {
      summary: { id:"archive", title:"Latest heading", provider:"chatgpt" },
      snapshot_id:"snap",
      capture: {
        title:"Original archived heading",
        captured_at:"2026-10-20",
        messages:[{ id:"msg", text:"Different message body", role:"assistant" }],
      },
    } as T;
    throw Error(command);
  };
  const { view, handle, search } = fixture(invoke);
  search("heading"); await delay(0);
  view.querySelector<HTMLButtonElement>("[data-library-archive]")!.click(); await delay(0);
  const preview = view.querySelector(".knowledge-library-preview")!.textContent!;
  assert.match(preview, /Snapshot: snap · title/);
  assert.match(preview, /Original archived heading/);
  assert.doesNotMatch(preview, /Different message body|Source preview unavailable/);
  handle.destroy();
});

import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";
import { JSDOM } from "jsdom";
import { test } from "node:test";
import { mountDocumentRootSetup } from "../src/document-roots.js";
import type { DocumentRoot } from "../src/document-roots.js";
import type { Invoke } from "../src/reader.js";

function setup(invoke: Invoke) {
  const dom = new JSDOM("<!doctype html><html><body><div id='app'></div></body></html>", {
    url: "https://mind-place.test/",
  });
  const root = dom.window.document.querySelector<HTMLElement>("#app")!;
  mountDocumentRootSetup(root, invoke);
  return { dom, root };
}

test("owner setup requires explicit grant and confirms permanent revocation", async () => {
  const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
  const roots: DocumentRoot[] = [];
  const invoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
    calls.push({ command, args });
    if (command === "list_document_roots") return [...roots] as T;
    if (command === "approve_document_root") {
      const found = roots.find((entry) => entry.id === args?.rootId)!;
      found.approval_state = "approved";
      return found as T;
    }
    if (command === "revoke_document_root") {
      const found = roots.find((entry) => entry.id === args?.rootId)!;
      found.approval_state = "revoked";
      return undefined as T;
    }
    if (command === "stage_document_root") {
      const item: DocumentRoot = {
        id: "root:v1:" + "a".repeat(64),
        canonical_path: String(args?.path),
        approval_state: "pending",
        virtual_prefix: "mpdoc://root/root:v1:" + "a".repeat(64),
      };
      roots.push(item);
      return item as T;
    }
    throw new Error("unexpected command");
  };
  const { dom, root } = setup(invoke);
  await delay(0);
  assert.match(root.textContent ?? "", /No document folders staged/);
  assert.match(root.textContent ?? "", /Staging only records a pending identity/);
  const input = root.querySelector<HTMLInputElement>("input[name='document-root-path']")!;
  input.value = "C:\\Users\\Test\\Documents";
  root.querySelector<HTMLFormElement>("form")!.dispatchEvent(new dom.window.Event("submit", { bubbles: true, cancelable: true }));
  await delay(0);
  await delay(0);
  assert.equal(calls.filter((call) => call.command === "stage_document_root").length, 1);
  assert.equal(calls.some((call) => /approve|scan|read_file|index_document/i.test(call.command)), false);
  assert.match(root.textContent ?? "", /PENDING/);
  assert.match(root.textContent ?? "", /No files were read or indexed/);
  assert.equal(input.value, "");
  const approve = root.querySelector<HTMLButtonElement>("[data-action='approve-root']")!;
  assert.ok(approve);
  assert.equal(calls.some((call) => call.command === "approve_document_root"), false);
  approve.click();
  await delay(0);
  assert.equal(calls.filter((call) => call.command === "approve_document_root").length, 1);
  const revoke = root.querySelector<HTMLButtonElement>("[data-action='revoke-root']")!;
  revoke.click();
  assert.match(revoke.textContent ?? "", /Confirm permanent revoke/);
  assert.equal(calls.some((call) => call.command === "revoke_document_root"), false);
  revoke.click();
  await delay(0);
  assert.equal(calls.filter((call) => call.command === "revoke_document_root").length, 1);
});

test("locked registry denies staging and does not render private results after disposal", async () => {
  let done!: (roots: DocumentRoot[]) => void;
  const pending = new Promise<DocumentRoot[]>((resolve) => { done = resolve; });
  const invoke: Invoke = async <T>(command: string) => {
    if (command === "list_document_roots") return pending as T;
    throw new Error("MP_LOCKED");
  };
  const { root } = setup(invoke);
  root.replaceChildren();
  done([{ id: "root:v1:" + "b".repeat(64), canonical_path: "C:\\secret", virtual_prefix: "mpdoc://root/root:v1:" + "b".repeat(64), approval_state: "approved" }]);
  await delay(0);
  assert.doesNotMatch(root.textContent ?? "", /secret/);
});

test("expired permanent-revoke confirmation requires a fresh two-click consent", async (t) => {
  let clock = Date.now();
  t.mock.method(Date, "now", () => clock);
  const roots: DocumentRoot[] = [{
    id: "root:v1:" + "c".repeat(64),
    canonical_path: "C:\\Users\\Test\\Documents",
    approval_state: "approved",
    virtual_prefix: "mpdoc://root/root:v1:" + "c".repeat(64),
  }];
  let revocations = 0;
  const invoke: Invoke = async <T>(command: string) => {
    if (command === "list_document_roots") return [...roots] as T;
    if (command === "revoke_document_root") {
      revocations++;
      roots[0].approval_state = "revoked";
      return undefined as T;
    }
    throw new Error("unexpected command");
  };
  const { root } = setup(invoke);
  await delay(0);
  const revoke = root.querySelector<HTMLButtonElement>("[data-action='revoke-root']")!;
  revoke.click();
  assert.match(revoke.textContent ?? "", /Confirm permanent revoke/);
  clock += 16_000;
  revoke.click();
  assert.equal(revocations, 0, "expired click must only restart the confirmation window");
  assert.match(revoke.textContent ?? "", /Confirm permanent revoke/);
  revoke.click();
  await delay(0);
  assert.equal(revocations, 1, "fresh second click confirms permanent revocation");
});


test("explicit document indexing and search render only inert text and clear on revoke", async () => {
  const id = "root:v1:" + "d".repeat(64);
  const roots: DocumentRoot[] = [{
    id, canonical_path: "C:\\Users\\Test\\Notes", approval_state: "approved",
    virtual_prefix: "mpdoc://root/" + id,
  }];
  const calls: string[] = [];
  const invoke: Invoke = async <T>(command: string) => {
    calls.push(command);
    if (command === "list_document_roots") return [...roots] as T;
    if (command === "refresh_document_root_index") return {
      root_id: id, scanned_files: 1, indexed_files: 1,
      skipped_files: 0, indexed_chunks: 1, generation: 1,
      auto_refresh_active: true,
    } as T;
    if (command === "search_document_index") return [{
      uri: "mpdoc://root/" + id + "/note.md", version_id: "docver:v1:test",
      section: "intro", snippet: "<img src=x onerror=alert(1)>", score: -1,
      extractor_revision: "mp-4e:1", warnings: ["<script>unsafe</script>"],
    }] as T;
    if (command === "pause_document_root_refresh") return undefined as T;
    if (command === "revoke_document_root") {
      roots[0].approval_state = "revoked";
      return undefined as T;
    }
    throw new Error("unexpected command");
  };
  const { dom, root } = setup(invoke);
  await delay(0);
  assert.equal(calls.includes("refresh_document_root_index"), false);
  root.querySelector<HTMLButtonElement>("[data-action='index-root']")!.click();
  await delay(0);
  assert.match(root.textContent ?? "", /Indexed 1 files/);
  assert.match(root.textContent ?? "", /Automatic 5-minute refresh enabled/);
  const searchForm = root.querySelector<HTMLFormElement>(".document-search-form")!;
  searchForm.querySelector("input")!.value = "notes";
  searchForm.dispatchEvent(new dom.window.Event("submit", { bubbles: true, cancelable: true }));
  await delay(0);
  assert.equal(calls.filter((call) => call === "search_document_index").length, 1);
  assert.match(root.textContent ?? "", /<img src=x onerror=alert\(1\)>/);
  assert.equal(root.querySelectorAll("img").length, 0, "document snippet must remain inert");
  assert.equal(root.querySelectorAll("script").length, 0, "parser warnings must remain inert");
  assert.match(root.textContent ?? "", /Extractor mp-4e:1/);
  root.querySelector<HTMLButtonElement>("[data-action='pause-root-refresh']")!.click();
  await delay(0);
  assert.equal(calls.filter((call) => call === "pause_document_root_refresh").length, 1);
  assert.match(root.textContent ?? "", /Periodic folder refresh paused/);
  const revoke = root.querySelector<HTMLButtonElement>("[data-action='revoke-root']")!;
  revoke.click();
  revoke.click();
  await delay(0);
  assert.equal(root.querySelectorAll(".document-search-results li").length, 0);
});

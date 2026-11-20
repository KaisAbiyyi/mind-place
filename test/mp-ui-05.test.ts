import assert from "node:assert/strict";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import { mountFilesVaultWorkspace } from "../src/files-vault-workspace.js";
import type { DocumentRoot } from "../src/document-roots.js";
import type { Invoke } from "../src/reader.js";

const tick = async () => { await new Promise(resolve => setTimeout(resolve, 0)); };
function render(vaultMode: "encrypted" | "legacy" | undefined, roots: DocumentRoot[] = []) {
  const dom = new JSDOM("<!doctype html><main id='view'></main>");
  const view = dom.window.document.querySelector<HTMLElement>("#view")!;
  const called: string[] = [];
  const invoke: Invoke = async <T>(command: string) => {
    called.push(command);
    if (command === "list_document_roots") return roots as T;
    if (command === "get_security_status") return { configured: true, locked: false, epoch: 7, retry_after_ms: 0, vault_mode: vaultMode } as T;
    throw Error("Unexpected native invocation: " + command);
  };
  return { dom, view, called, handle: mountFilesVaultWorkspace(view, invoke) };
}

test("UI-05 reports native encryption state separately from source files and does not scan on mount", async () => {
  const { dom, view, called, handle } = render("encrypted");
  await tick();
  assert.match(view.textContent!, /encrypted app vault mode/i);
  assert.match(view.textContent!, /not an encryption claim for original documents/i);
  assert.ok(view.querySelector("[data-route='settings']"));
  assert.ok(view.querySelector(".document-authorize-dialog"));
  assert.equal(called.includes("refresh_document_root_index"), false);
  assert.equal(called.includes("approve_document_root"), false);
  handle.destroy();
  assert.equal(view.textContent, "");
  dom.window.close();
});

test("UI-05 folder filter and inspection are actual authorized-root data, not fabricated totals", async () => {
  const roots: DocumentRoot[] = [
    { id: "r1", canonical_path: "C:\\Work", virtual_prefix: "mpdoc://root/r1", approval_state: "approved" },
    { id: "r2", canonical_path: "C:\\Pending", virtual_prefix: "mpdoc://root/r2", approval_state: "pending" },
  ];
  const { dom, view, called, handle } = render("legacy", roots);
  await tick();
  assert.equal(view.querySelectorAll(".document-root-card").length, 2);
  const inspect = view.querySelector<HTMLButtonElement>("[data-action='inspect-document-root']")!;
  inspect.click();
  assert.match(view.querySelector(".document-inspector")!.textContent!, /historical index data is not exposed/i);
  const filter = view.querySelector<HTMLSelectElement>(".document-root-toolbar select")!;
  filter.value = "pending"; filter.dispatchEvent(new dom.window.Event("change"));
  await tick();
  assert.equal(view.querySelectorAll(".document-root-card").length, 1);
  assert.match(view.querySelector(".document-root-card")!.textContent!, /Pending/);
  assert.equal(called.includes("refresh_document_root_index"), false);
  assert.equal(called.includes("approve_document_root"), false);
  handle.destroy(); dom.window.close();
});

test("UI-05 teardown prevents late vault status disclosure", async () => {
  let finish!: (v: unknown) => void;
  const pending = new Promise(resolve => { finish = resolve; });
  const dom = new JSDOM("<!doctype html><div id='view'></div>");
  const view = dom.window.document.querySelector<HTMLElement>("#view")!;
  const invoke: Invoke = async <T>(command: string) =>
    command === "get_security_status" ? pending as T : [] as T;
  const handle = mountFilesVaultWorkspace(view, invoke);
  handle.destroy();
  finish({ configured: true, locked: false, epoch: 1, retry_after_ms: 0, vault_mode: "encrypted" });
  await tick();
  assert.equal(view.textContent, "");
  dom.window.close();
});

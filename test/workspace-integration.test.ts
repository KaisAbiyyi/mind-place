import assert from "node:assert/strict";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import { WORKSPACES, bindWorkspaceNavigationKeyboard, createWorkspaceNavigation } from "../src/knowledge-os.js";
import { mountReader, type Invoke } from "../src/reader.js";

const tick = () => new Promise<void>(resolve => setTimeout(resolve, 0));

test("workspace keyboard navigation moves focus without changing active content", () => {
  const dom = new JSDOM("<!doctype html><html><body></body></html>");
  const nav = createWorkspaceNavigation(dom.window.document);
  dom.window.document.body.append(nav);
  const dispose = bindWorkspaceNavigationKeyboard(nav);
  try {
    assert.equal(WORKSPACES.length, 10);
    const buttons = [...nav.querySelectorAll<HTMLButtonElement>("button[data-workspace]")];
    assert.equal(buttons[0]?.dataset.workspace, "conversations");
    assert.equal(buttons[1]?.dataset.workspace, "library");
    buttons[0].focus();
    buttons[0].dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true, cancelable: true }));
    assert.equal(dom.window.document.activeElement, buttons[1]);
    buttons[1].dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "End", bubbles: true, cancelable: true }));
    assert.equal(dom.window.document.activeElement, buttons[buttons.length - 1]);
    assert.equal(nav.querySelector('[aria-current="page"]')?.getAttribute("data-workspace"), "conversations");
  } finally {
    dispose();
    dom.window.close();
  }
});

test("Library, Memories, Hive and Health preserve truthful active navigation and labeled views", async () => {
  const dom = new JSDOM("<!doctype html><html><body><div id='app'></div></body></html>", { url: "https://archive.test/" });
  const root = dom.window.document.getElementById("app")!;
  const invoke: Invoke = async <T>(command: string) => {
    if (command === "list_conversations" || command === "list_memory_inspection") return [] as T;
    if (command === "get_security_status") return { configured: true, locked: false, epoch: 5 } as T;
    if (command === "get_archive_index_stats") return { sources: 0, revisions: 0, chunks: 0, generation: 1 } as T;
    if (command === "get_local_mcp_status" || command === "get_hosted_tunnel_status") return { running: false, connection_verified: false } as T;
    throw new Error("Unavailable mock: " + command);
  };
  const app = mountReader({ root, invoke });
  try {
    await app.ready;
    const open = (id: string) => {
      root.querySelector<HTMLButtonElement>(`[data-workspace="${id}"]`)!.click();
      assert.equal(root.querySelector('[aria-current="page"]')?.getAttribute("data-workspace"), id);
      assert.equal(root.querySelector("#reader-view")?.getAttribute("aria-label"), WORKSPACES.find(x => x.id === id)?.label + " workspace");
      assert.equal(dom.window.document.activeElement?.tagName, "H1");
    };
    open("library");
    assert.match(root.querySelector("#reader-view")?.textContent ?? "", /Search your library/);
    open("memories");
    await tick();
    open("graph");
    await tick();
    open("health");
    await tick();
    root.querySelector<HTMLButtonElement>('[data-workspace="library"]')!.click();
    root.querySelector<HTMLButtonElement>('[data-library-action="close"]')!.click();
    assert.equal(root.querySelector('[aria-current="page"]')?.getAttribute("data-workspace"), "conversations");
  } finally {
    app.destroy(); dom.window.close();
  }
});

test("setup and documents modal return to the previous workspace and never steal slash focus", async () => {
  const dom = new JSDOM("<!doctype html><html><body><div id='app'></div></body></html>", { url: "https://archive.test/" });
  const root = dom.window.document.getElementById("app")!;
  const invoke: Invoke = async <T>(command: string) => {
    if (command === "list_conversations") return [] as T;
    if (command === "get_setup") throw new Error("No setup on synthetic fixture");
    throw new Error(command);
  };
  const app = mountReader({ root, invoke });
  try {
    await app.ready;
    root.querySelector<HTMLButtonElement>('[data-workspace="library"]')!.click();
    const setup = root.querySelector<HTMLButtonElement>('[data-workspace="connections"]')!;
    setup.click();
    assert.equal(root.querySelector('[aria-current="page"]')?.getAttribute("data-workspace"), "connections");
    const dialog = root.querySelector<HTMLDialogElement>("#setup-dialog")!;
    assert.equal(dialog.hasAttribute("open"), true);
    root.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "/", bubbles: true, cancelable: true }));
    assert.notEqual(dom.window.document.activeElement?.id, "archive-search");
    root.querySelector<HTMLButtonElement>('[data-action="close-setup"]')!.click();
    assert.equal(root.querySelector('[aria-current="page"]')?.getAttribute("data-workspace"), "library");
    assert.equal(dom.window.document.activeElement, setup);
    const documents = root.querySelector<HTMLButtonElement>('[data-workspace="documents"]')!;
    documents.click();
    assert.equal(root.querySelector('[aria-current="page"]')?.getAttribute("data-workspace"), "documents");
    dialog.dispatchEvent(new dom.window.Event("cancel", { cancelable: true }));
    assert.equal(root.querySelector('[aria-current="page"]')?.getAttribute("data-workspace"), "library");
    assert.equal(dialog.hasAttribute("open"), false);
  } finally {
    app.destroy(); dom.window.close();
  }
});

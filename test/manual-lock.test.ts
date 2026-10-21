import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";
import { JSDOM } from "jsdom";
import { test } from "node:test";
import { mountReader, type Invoke } from "../src/reader.js";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const unlocked = { configured: true, locked: false, epoch: 1, retry_after_ms: 0 };
const locked = { ...unlocked, locked: true, epoch: 2 };
const setup = { platform: "windows", archive_path: "fixture-archive", extension_path: "fixture-extension", native_host_name: "com.mindplace.capture", chrome_registered: true, edge_registered: false, legacy_available: false };
function fixture() {
  const dom = new JSDOM("<body><div id='app'></div></body>", { url: "https://lock.test/" });
  const root = dom.window.document.querySelector<HTMLElement>("#app")!;
  return { dom, root };
}
async function mountApp(root: HTMLElement, invoke: Invoke) {
  const module = await import("../src/app.js");
  await module.mountApplication({ root, invoke, clipboard: { writeText: async () => {} } });
}
function lockButton(root: HTMLElement, dialog = false) {
  const button = root.querySelector<HTMLButtonElement>(dialog ? "#setup-dialog [data-action='lock']" : ".reader-toolbar [data-action='lock']");
  assert.ok(button, "owner Lock must be reachable from the toolbar and setup");
  return button;
}
test("owner Lock is reachable from both reader toolbar and open setup", async () => {
  const { dom, root } = fixture();
  let locks = 0;
  const invoke: Invoke = async <T>(command: string) => {
    if (command === "list_conversations") return [] as T;
    if (command === "get_setup") return setup as T;
    if (command === "list_local_mcp_clients") return [] as T;
    return { running: false } as T;
  };
  const reader = mountReader({ root, invoke, onLock: () => { locks++; } });
  try {
    await reader.ready;
    lockButton(root).click();
    root.querySelector<HTMLButtonElement>("[data-action='open-setup']")!.click();
    await delay(0);
    lockButton(root, true).click();
    assert.equal(locks, 2);
  } finally { reader.destroy(); dom.window.close(); }
});

test("Lock clears archive and setup secrets before native acknowledgement and ignores late credential completion", async () => {
  const { dom, root } = fixture();
  const pendingLock = deferred<typeof locked>();
  const pendingCredential = deferred<{ client_id: string; credential: string }>();
  let security = unlocked;
  let locks = 0;
  const invoke: Invoke = async <T>(command: string) => {
    if (command === "get_security_status") return security as T;
    if (command === "list_conversations" || command === "list_local_mcp_clients") return [] as T;
    if (command === "refresh_archive_index") return {} as T;
    if (command === "get_setup") return setup as T;
    if (command === "issue_local_mcp_reader") return await pendingCredential.promise as T;
    if (command === "lock_security") { locks++; security = locked; return await pendingLock.promise as T; }
    return { running: false } as T;
  };
  const previous = { document: globalThis.document, Element: globalThis.Element };
  Object.assign(globalThis, { document: dom.window.document, Element: dom.window.Element });
  try {
    await mountApp(root, invoke);
    await delay(0);
    root.querySelector<HTMLButtonElement>("[data-action='open-setup']")!.click();
    await delay(0);
    root.querySelector<HTMLInputElement>("#hosted-runtime-key")!.value = "fixture-runtime-secret";
    root.querySelector<HTMLButtonElement>("[data-mcp-action='issue']")!.click();
    const staleLock = lockButton(root, true);
    staleLock.click();
    staleLock.click();
    assert.equal(locks, 1);
    assert.equal(root.querySelector("#setup-dialog"), null);
    assert.equal(root.querySelector(".app-shell"), null);
    assert.equal(root.querySelector("input[type='password']"), null);
    assert.match(root.textContent ?? "", /locking/i);
    pendingCredential.resolve({ client_id: "fixture", credential: "fixture-one-time-credential" });
    pendingLock.resolve(locked);
    await delay(0);
    assert.match(root.textContent ?? "", /Unlock Mind Place/);
    assert.doesNotMatch(root.textContent ?? "", /fixture-one-time-credential|fixture-runtime-secret|fixture-archive/);
    assert.equal(root.ownerDocument.activeElement?.id, "security-secret");
  } finally { Object.assign(globalThis, previous); dom.window.close(); }
});

test("Lock failure stays private and retry requires a confirmed locked result", async () => {
  const { dom, root } = fixture();
  let attempts = 0;
  const invoke: Invoke = async <T>(command: string) => {
    if (command === "get_security_status") return (attempts >= 3 ? locked : unlocked) as T;
    if (command === "list_conversations") return [] as T;
    if (command === "refresh_archive_index") return {} as T;
    if (command === "lock_security") {
      attempts++;
      if (attempts === 1) throw new Error("private-native-error");
      return (attempts === 2 ? unlocked : locked) as T;
    }
    throw new Error("unexpected command");
  };
  try {
    await mountApp(root, invoke);
    await delay(0);
    lockButton(root).click();
    await delay(0);
    assert.equal(root.querySelector(".app-shell"), null);
    assert.equal(root.querySelector("#security-form"), null);
    assert.doesNotMatch(root.textContent ?? "", /private-native-error/);
    assert.match(root.textContent ?? "", /could not confirm|not confirmed/i);
    root.querySelector<HTMLButtonElement>("[data-action='retry-lock']")!.click();
    await delay(0);
    assert.equal(root.querySelector("#security-form"), null);
    root.querySelector<HTMLButtonElement>("[data-action='retry-lock']")!.click();
    await delay(0);
    assert.equal(attempts, 3);
    assert.ok(root.querySelector("#security-form"));
  } finally { dom.window.close(); }
});

test("late archive result cannot repopulate the locked UI and fresh unlock never starts MCP", async () => {
  const { dom, root } = fixture();
  const pendingList = deferred<unknown[]>();
  let security = unlocked;
  let lists = 0;
  const commands: string[] = [];
  const invoke: Invoke = async <T>(command: string) => {
    commands.push(command);
    if (command === "get_security_status") return security as T;
    if (command === "list_conversations") return (lists++ === 0 ? await pendingList.promise : []) as T;
    if (command === "refresh_archive_index") return {} as T;
    if (command === "lock_security") { security = locked; return locked as T; }
    if (command === "unlock_security") { security = { ...unlocked, epoch: 3 }; return security as T; }
    throw new Error("unexpected command");
  };
  try {
    await mountApp(root, invoke);
    lockButton(root).click();
    await delay(0);
    pendingList.resolve([{ id: "a".repeat(64), title: "late private title", conversation_id: "fixture", provider: "chatgpt", source_url: "https://chatgpt.com/c/fixture", captured_at: "2026-10-21T00:00:00Z", message_count: 0, snapshot_count: 1, completeness: "partial", warnings: [] }]);
    await delay(0);
    assert.doesNotMatch(root.textContent ?? "", /late private title/);
    const input = root.querySelector<HTMLInputElement>("#security-secret")!;
    input.value = "fixture-password";
    root.querySelector<HTMLFormElement>("#security-form")!.dispatchEvent(new dom.window.Event("submit", { bubbles: true, cancelable: true }));
    await delay(0);
    assert.ok(root.querySelector(".app-shell"));
    assert.equal(root.querySelector("#security-form"), null);
    assert.equal(commands.includes("start_local_mcp"), false);
    assert.equal(commands.includes("start_hosted_tunnel"), false);
    assert.doesNotMatch(root.textContent ?? "", /late private title/);
  } finally { dom.window.close(); }
});

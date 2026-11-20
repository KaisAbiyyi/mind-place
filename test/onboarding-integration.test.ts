import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";
import { JSDOM } from "jsdom";
import { test } from "node:test";
import { mountReader, type Invoke } from "../src/reader.js";
import { createMemoryProgressStore } from "../src/onboarding-workspace.js";

const unlocked = { configured: true, locked: false, epoch: 1, retry_after_ms: 0, vault_mode: "legacy" };
const setupInfo = { platform: "windows", archive_path: "fixture-archive", extension_path: "fixture-extension", native_host_name: "com.mindplace.capture", host_path: "fixture-host", extension_id: "fixture-extension-id", registered_browsers: [], legacy_available: false };
const summary = { id: "c1", conversation_id: "c1", provider: "chatgpt", title: "First chat", source_url: "https://chatgpt.com/c/c1", captured_at: "2026-11-01T00:00:00Z", message_count: 1, snapshot_count: 1, completeness: "complete", warnings: [] };
const conversation = { summary, snapshot_id: "snap-1", capture: { schema_version: 1, provider: "chatgpt", conversation_id: "c1", source_url: summary.source_url, title: "First chat", captured_at: "2026-11-01T00:00:00Z", capture_method: "manual-dom", completeness: "complete", warnings: [], messages: [{ id: "m1", role: "user", text: "Hello archive", format: "plain", attachments: [] }] } };

function fixture() {
  const dom = new JSDOM("<body><div id='app'></div></body>", { url: "https://onboarding.test/" });
  return { dom, root: dom.window.document.querySelector<HTMLElement>("#app")! };
}
const backend = (archive: unknown[] = [summary]): Invoke => async <T>(command: string) => {
  switch (command) {
    case "get_security_status": return unlocked as T;
    case "list_conversations": return archive as T;
    case "list_snapshots": return [] as T;
    case "read_conversation": return conversation as T;
    case "get_setup": return setupInfo as T;
    case "list_local_mcp_clients": return [] as T;
    case "refresh_archive_index": return {} as T;
    default: return { running: false } as T;
  }
};
const wizard = (root: HTMLElement) => root.querySelector<HTMLElement>(".onboarding-workspace");

test("a newly created secret opens Setup & Onboarding once the archive view is ready", async () => {
  const { dom, root } = fixture();
  const store = createMemoryProgressStore();
  const reader = mountReader({ root, invoke: backend(), openOnboarding: true, onboardingStore: store });
  try {
    await reader.ready; await delay(0);
    assert.ok(wizard(root), "wizard should be mounted for the first-run session");
    assert.equal(root.querySelector("[data-route='setup-onboarding']")?.getAttribute("aria-current"), "page");
    assert.match(root.querySelector(".onboarding-workspace h2")!.textContent!, /Welcome to Mind Place/);
  } finally { reader.destroy(); dom.window.close(); }
});

test("returning and finished users are never forced into the wizard", async () => {
  for (const options of [{ openOnboarding: false }, { openOnboarding: true }]) {
    const { dom, root } = fixture();
    const store = createMemoryProgressStore(options.openOnboarding
      ? { version: 1, step: "welcome", providersSkipped: false, finishedAt: "2026-11-01T00:00:00.000Z" } : null);
    const reader = mountReader({ root, invoke: backend(), ...options, onboardingStore: store });
    try {
      await reader.ready; await delay(0);
      assert.equal(wizard(root), null);
    } finally { reader.destroy(); dom.window.close(); }
  }
});

test("the sidebar route opens the wizard, other routes unmount it, and Finish returns to Overview", async () => {
  const { dom, root } = fixture();
  const reader = mountReader({ root, invoke: backend(), onboardingStore: createMemoryProgressStore() });
  try {
    await reader.ready;
    root.querySelector<HTMLButtonElement>("[data-route='setup-onboarding']")!.click();
    assert.ok(wizard(root));
    root.querySelector<HTMLButtonElement>("[data-route='conversations']")!.click();
    await delay(0);
    assert.equal(wizard(root), null, "leaving the route must unmount the wizard");
    root.querySelector<HTMLButtonElement>("[data-route='setup-onboarding']")!.click();
    root.querySelector<HTMLButtonElement>("[data-step='first-result']")!.click();
    await delay(0); await delay(0);
    root.querySelector<HTMLButtonElement>("[data-focus-key='finish']")!.click();
    assert.equal(wizard(root), null);
    assert.equal(root.querySelector("[data-route='overview']")?.getAttribute("aria-current"), "page");
  } finally { reader.destroy(); dom.window.close(); }
});

test("the wizard stays mounted behind the capture dialog and resumes after provider destinations", async () => {
  const { dom, root } = fixture();
  const store = createMemoryProgressStore();
  const reader = mountReader({ root, invoke: backend(), onboardingStore: store });
  try {
    await reader.ready;
    root.querySelector<HTMLButtonElement>("[data-route='setup-onboarding']")!.click();
    root.querySelector<HTMLButtonElement>("[data-step='providers']")!.click();
    root.querySelector<HTMLButtonElement>("[data-focus-key='open-details']")!.click();
    await delay(0); await delay(0);
    assert.ok(root.querySelector<HTMLDialogElement>("#setup-dialog")!.hasAttribute("open"));
    assert.ok(wizard(root), "dialog from the wizard must not unmount it");
    root.querySelector<HTMLButtonElement>("[data-action='close-setup']")!.click();
    assert.ok(wizard(root));
    // Provider destination replaces the wizard; closing returns to the saved step.
    root.querySelector<HTMLButtonElement>("[data-focus-key='open-providers']")!.click();
    await delay(0); await delay(0);
    assert.equal(wizard(root), null);
    root.querySelector<HTMLButtonElement>("[data-action='close-setup']")!.click();
    await delay(0);
    assert.ok(wizard(root));
    assert.equal(wizard(root)!.querySelector("[aria-current='step']")?.getAttribute("data-step"), "providers");
  } finally { reader.destroy(); dom.window.close(); }
});

test("Open this conversation from the first result opens the verified revision in Conversations", async () => {
  const { dom, root } = fixture();
  const reader = mountReader({ root, invoke: backend(), onboardingStore: createMemoryProgressStore() });
  try {
    await reader.ready;
    root.querySelector<HTMLButtonElement>("[data-route='setup-onboarding']")!.click();
    root.querySelector<HTMLButtonElement>("[data-step='first-result']")!.click();
    await delay(0); await delay(0);
    root.querySelector<HTMLButtonElement>("[data-focus-key='read-latest']")!.click();
    await delay(0); await delay(0);
    assert.ok(root.querySelector("[data-verified='true']"));
    root.querySelector<HTMLButtonElement>("[data-focus-key='open-conversation']")!.click();
    await delay(0); await delay(0); await delay(0);
    assert.equal(wizard(root), null);
    assert.match(root.querySelector("#reader-view")!.textContent!, /Hello archive/);
  } finally { reader.destroy(); dom.window.close(); }
});

test("destroying the reader (Lock) clears the wizard and any verified excerpt", async () => {
  const { dom, root } = fixture();
  const reader = mountReader({ root, invoke: backend(), onboardingStore: createMemoryProgressStore() });
  await reader.ready;
  root.querySelector<HTMLButtonElement>("[data-route='setup-onboarding']")!.click();
  root.querySelector<HTMLButtonElement>("[data-step='first-result']")!.click();
  await delay(0); await delay(0);
  root.querySelector<HTMLButtonElement>("[data-focus-key='read-latest']")!.click();
  await delay(0); await delay(0);
  assert.match(root.textContent!, /Hello archive/);
  reader.destroy();
  assert.equal(root.textContent, "");
  dom.window.close();
});

test("application: only the session that created the secret opens the wizard", async () => {
  for (const firstRun of [true, false]) {
    const { dom, root } = fixture();
    const base = backend();
    let configured = !firstRun;
    const invoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "get_security_status") return (configured ? { ...unlocked, locked: true } : { configured: false, locked: true, epoch: 0, retry_after_ms: 0 }) as T;
      if (command === "initialize_security" || command === "unlock_security") { configured = true; return unlocked as T; }
      return base<T>(command, args);
    };
    const previous = { document: globalThis.document, Element: globalThis.Element };
    Object.assign(globalThis, { document: dom.window.document, Element: dom.window.Element });
    try {
      const module = await import("../src/app.js");
      await module.mountApplication({ root, invoke, clipboard: { writeText: async () => {} } });
      const input = root.querySelector<HTMLInputElement>("#security-secret")!;
      input.value = "correct horse battery";
      if (firstRun) root.querySelector<HTMLInputElement>("#security-secret-confirm")!.value = "correct horse battery";
      root.querySelector<HTMLFormElement>("#security-form")!.dispatchEvent(new dom.window.Event("submit", { bubbles: true, cancelable: true }));
      for (let i = 0; i < 8; i++) await delay(0);
      assert.equal(Boolean(wizard(root)), firstRun);
    } finally { Object.assign(globalThis, previous); dom.window.close(); }
  }
});

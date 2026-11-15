import assert from "node:assert/strict";
import { test } from "node:test";
import { setTimeout as delay } from "node:timers/promises";
import { JSDOM } from "jsdom";
import type { Invoke } from "../src/reader.js";
import { createI18nController } from "../src/ui2/i18n/index.js";
import { UI2_DESTINATIONS } from "../src/ui2/ui-kit/index.js";
import { isUi2ShellEnabled, setUi2ShellEnabled, UI2_SHELL_FEATURE_FLAG } from "../src/ui2/shell/feature-flag.js";
import { fenceUi2EpochInvoke, mountUi2Shell } from "../src/ui2/shell/shell.js";
import { readUi2Hash, resolveUi2Route, UI2_SHELL_DESTINATIONS, ui2HashForPath } from "../src/ui2/shell/routes.js";
import { mountApplication } from "../src/app.js";

function fixture(hash = "") {
  const dom = new JSDOM("<!doctype html><html><body><div id='app'></div></body></html>", {
    url: "https://shell.test/" + hash,
  });
  return { dom, root: dom.window.document.getElementById("app")! };
}

test("MP-UI2-04: seven routes are fixed, nested breadcrumbs rooted at Home, invalid routes fail closed", () => {
  assert.deepEqual(UI2_SHELL_DESTINATIONS.map(entry => entry.destination), UI2_DESTINATIONS);
  assert.deepEqual(UI2_SHELL_DESTINATIONS.map(entry => entry.path), [
    "/home", "/memory", "/archive", "/constellation", "/sources", "/connections", "/settings",
  ]);
  assert.deepEqual(resolveUi2Route("/archive/chat").breadcrumbs.map(item => item.path), [
    "/home", "/archive", "/archive/chat",
  ]);
  assert.deepEqual(resolveUi2Route("/settings/appearance").breadcrumbs.map(item => item.path), [
    "/home", "/settings", "/settings/appearance",
  ]);
  assert.equal(readUi2Hash(ui2HashForPath("/memory/detail")).path, "/memory/detail");
  assert.equal(resolveUi2Route("/memory/secret-id").recognized, false);
  assert.equal(resolveUi2Route("//evil.example/").recognized, false);
  assert.equal(resolveUi2Route("/memory/detail").available, false);
});

test("MP-UI2-04: dev/owner feature flag defaults v1 and requires exact enable value", () => {
  const map = new Map<string, string>();
  const store = {
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => { map.set(key, value); },
  };
  assert.equal(isUi2ShellEnabled(store), false);
  assert.equal(setUi2ShellEnabled(true, store), true);
  assert.equal(map.get(UI2_SHELL_FEATURE_FLAG), "enabled");
  assert.equal(isUi2ShellEnabled(store), true);
  assert.equal(setUi2ShellEnabled(false, store), true);
  assert.equal(isUi2ShellEnabled(store), false);
  map.set(UI2_SHELL_FEATURE_FLAG, "true");
  assert.equal(isUi2ShellEnabled(store), false);
  assert.equal(isUi2ShellEnabled({ getItem: () => { throw new Error("denied"); }, setItem() {} }), false);
  assert.equal(setUi2ShellEnabled(true, { getItem: () => null, setItem: () => { throw new Error("denied"); } }), false);
});

test("MP-UI2-04: navigation and clickable breadcrumbs, language switch, a11y keyboard and disposal", () => {
  const { dom, root } = fixture();
  const locale = createI18nController({ initialLocale: "en" });
  let lockCalls = 0;
  const shell = mountUi2Shell(root, { i18n: locale, onLock: () => { lockCalls++; } });
  const doc = dom.window.document;
  assert.equal(doc.querySelectorAll(".ui2-shell-nav-button").length, 7);
  assert.equal(doc.querySelector(".ui2-shell-brand")?.textContent, "m", "logo-only sidebar header");
  assert.match(doc.querySelector(".ui2-shell-placeholder")?.textContent ?? "", /not ready/i);

  const memory = doc.querySelector<HTMLButtonElement>('[data-ui2-route="/memory"]')!;
  memory.click();
  assert.equal(shell.getRoute().destination, "memory");
  assert.equal(doc.querySelector('[aria-current="page"].ui2-breadcrumb-current')?.textContent, "Memory");
  doc.querySelector<HTMLButtonElement>(".ui2-breadcrumb-link")!.click();
  assert.equal(shell.getRoute().path, "/home");

  shell.navigate("/settings/appearance");
  assert.deepEqual(shell.getRoute().breadcrumbs.map(item => item.path), ["/home", "/settings", "/settings/appearance"]);
  doc.querySelectorAll<HTMLButtonElement>(".ui2-breadcrumb-link")[1]!.click();
  assert.equal(shell.getRoute().path, "/settings");

  const first = doc.querySelector<HTMLButtonElement>('[data-ui2-route="/home"]')!;
  first.focus();
  first.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
  assert.equal(doc.activeElement?.getAttribute("data-ui2-route"), "/memory");
  first.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "End", bubbles: true, cancelable: true }));
  assert.equal(doc.activeElement?.getAttribute("data-ui2-route"), "/settings");

  doc.querySelector<HTMLButtonElement>('[data-ui2-seg-value="id"]')!.click();
  assert.equal(locale.getLocale(), "id");
  assert.equal(doc.documentElement.lang, "id");
  assert.equal(doc.querySelector('[data-ui2-route="/home"]')?.getAttribute("aria-label"), "Beranda");
  assert.match(doc.querySelector(".ui2-shell-placeholder")?.textContent ?? "", /belum tersedia/i);

  doc.querySelector<HTMLButtonElement>('[data-ui2-control="lock"]')!.click();
  assert.equal(lockCalls, 1);
  shell.destroy();
  assert.equal(root.childElementCount, 0);
  doc.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "k", ctrlKey: true, bubbles: true }));
  assert.equal(root.childElementCount, 0);
  dom.window.close();
});

test("MP-UI2-04: Ctrl/Meta+K opens honest unavailable search, Escape restores focus", () => {
  const { dom, root } = fixture();
  const shell = mountUi2Shell(root, { onLock() {} });
  const doc = dom.window.document;
  const open = doc.querySelector<HTMLButtonElement>('[data-ui2-control="search"]')!;
  open.focus();
  doc.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "k", ctrlKey: true, bubbles: true, cancelable: true }));
  const dialog = doc.querySelector<HTMLElement>('[role="dialog"][aria-modal="true"]')!;
  assert.equal(dialog.hidden, false);
  assert.match(dialog.textContent ?? "", /not ready/i);
  dialog.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
  assert.equal(doc.activeElement, open);
  shell.destroy();
  dom.window.close();
});

test("MP-UI2-04: mobile nav overlay, Escape focus return, desktop collapse", () => {
  const { dom, root } = fixture();
  let mobile = true;
  Object.defineProperty(dom.window, "matchMedia", { configurable: true, value: () => ({ matches: mobile }) });
  const shell = mountUi2Shell(root, { onLock() {} });
  const doc = dom.window.document;
  const toggle = doc.querySelector<HTMLButtonElement>('[data-ui2-control="sidebar"]')!;
  const sidebar = root.querySelector<HTMLElement>(".ui2-shell-sidebar")!;
  assert.equal(sidebar.inert, true, "closed mobile rail must not receive keyboard focus");
  assert.equal(sidebar.getAttribute("aria-hidden"), "true");
  toggle.click();
  assert.equal(sidebar.inert, false);
  assert.equal(root.querySelector(".ui2-shell")?.getAttribute("data-mobile-open"), "true");
  assert.equal(doc.activeElement?.getAttribute("data-ui2-route"), "/home");
  doc.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
  assert.equal(root.querySelector(".ui2-shell")?.getAttribute("data-mobile-open"), "false");
  assert.equal(sidebar.inert, true);
  assert.equal(doc.activeElement, toggle);
  mobile = false;
  dom.window.dispatchEvent(new dom.window.Event("resize"));
  assert.equal(sidebar.inert, false);
  assert.equal(sidebar.hasAttribute("aria-hidden"), false);
  toggle.click();
  assert.equal(root.querySelector(".ui2-shell")?.getAttribute("data-collapsed"), "true");
  toggle.click();
  assert.equal(root.querySelector(".ui2-shell")?.getAttribute("data-collapsed"), "false");
  shell.destroy();
  dom.window.close();
});

test("MP-UI2-04: old-epoch async completions reject and cannot mutate new session", async () => {
  let current = true;
  let resolveRead!: (value: string) => void;
  const pending = new Promise<string>(resolve => { resolveRead = resolve; });
  let calls = 0;
  const invoke: Invoke = async <T>() => { calls++; return await pending as T; };
  const fenced = fenceUi2EpochInvoke(invoke, () => current);
  const request = fenced("read_stub");
  current = false;
  resolveRead("sensitive-stale-data");
  await assert.rejects(request, /MP_UI_SESSION_EXPIRED/);
  await assert.rejects(fenced("read_stub"), /MP_UI_SESSION_EXPIRED/);
  assert.equal(calls, 1, "expired calls never reach native backend");
});

test("MP-UI2-04: locked startup never mounts preview; verified unlock selects preview, Lock returns gate", async () => {
  const { dom, root } = fixture();
  dom.window.localStorage.setItem(UI2_SHELL_FEATURE_FLAG, "enabled");
  // app.ts uses the runtime storage API, which is typically the active browser window.
  const originalStorage = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
  Object.defineProperty(globalThis, "localStorage", { configurable: true, value: dom.window.localStorage });
  let locked = true;
  const commands: string[] = [];
  const invoke: Invoke = async <T>(command: string) => {
    commands.push(command);
    if (command === "get_security_status") return { configured: true, locked, epoch: locked ? 0 : 1, retry_after_ms: 0 } as T;
    if (command === "unlock_security") { locked = false; return { configured: true, locked: false, epoch: 1, retry_after_ms: 0 } as T; }
    if (command === "lock_security") { locked = true; return { configured: true, locked: true, epoch: 2, retry_after_ms: 0 } as T; }
    throw new Error("Unexpected command " + command);
  };
  try {
    await mountApplication({ root, invoke });
    assert.ok(root.querySelector("#security-form"));
    assert.equal(root.querySelector(".ui2-shell"), null);
    root.querySelector<HTMLInputElement>("#security-secret")!.value = "synthetic-unlock-password";
    root.querySelector<HTMLFormElement>("#security-form")!.dispatchEvent(
      new dom.window.Event("submit", { bubbles: true, cancelable: true }),
    );
    await delay(50);
    assert.ok(root.querySelector(".ui2-shell"), "preview mounts only after security unlock");
    root.querySelector<HTMLButtonElement>('[data-ui2-control="lock"]')!.click();
    await delay(10);
    assert.ok(root.querySelector("#security-form"), "after native lock confirmation the original gate is back");
    assert.equal(root.querySelector(".ui2-shell"), null);
    assert.deepEqual(commands, ["get_security_status", "unlock_security", "lock_security", "get_security_status"]);
    assert.equal(dom.window.localStorage.getItem(UI2_SHELL_FEATURE_FLAG), "enabled");
  } finally {
    if (originalStorage) Object.defineProperty(globalThis, "localStorage", originalStorage);
    else delete (globalThis as { localStorage?: Storage }).localStorage;
    dom.window.close();
  }
});

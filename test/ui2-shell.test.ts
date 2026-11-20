import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
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
  const previousDocumentLanguage = dom.window.document.documentElement.lang;
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
  assert.equal(doc.documentElement.lang, previousDocumentLanguage, "disposal restores the exact original document language");
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

test("MP-UI2-04: one compiled app selects legacy v1 by default and v2 only after opt-in", async () => {
  const globalStorage = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
  const previous = fixture();
  const preview = fixture();
  const originalGlobalStorage = globalStorage;
  const invoke: Invoke = async <T>(command: string) => {
    if (command === "get_security_status") {
      return { configured: true, locked: false, epoch: 4, retry_after_ms: 0 } as T;
    }
    if (command === "list_conversations") return [] as T;
    throw new Error("Unexpected test command " + command);
  };
  try {
    Object.defineProperty(globalThis, "localStorage", { configurable: true, value: previous.dom.window.localStorage });
    await mountApplication({ root: previous.root, invoke });
    await delay(10);
    assert.ok(previous.root.querySelector("#conversation-list"), "classic reader remains the default");
    assert.equal(previous.root.querySelector(".ui2-shell"), null);

    preview.dom.window.localStorage.setItem(UI2_SHELL_FEATURE_FLAG, "enabled");
    Object.defineProperty(globalThis, "localStorage", { configurable: true, value: preview.dom.window.localStorage });
    await mountApplication({ root: preview.root, invoke });
    await delay(20);
    assert.ok(preview.root.querySelector(".ui2-shell"), "same bundle selects flagged shell");
    assert.equal(preview.root.querySelector("#conversation-list"), null, "preview never mounts classic reader simultaneously");
  } finally {
    if (originalGlobalStorage) Object.defineProperty(globalThis, "localStorage", originalGlobalStorage);
    else delete (globalThis as { localStorage?: Storage }).localStorage;
    previous.dom.window.close();
    preview.dom.window.close();
  }
});

test("MP-UI2-04: fresh install preserves classic first-run onboarding under v2 flag, then subsequent unlock mounts v2 with EN/ID and lock safety", async () => {
  const { dom, root } = fixture();
  dom.window.document.documentElement.lang = "en";
  dom.window.localStorage.setItem(UI2_SHELL_FEATURE_FLAG, "enabled");
  const originalStorage = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
  const previousGlobals = { document: globalThis.document, Element: globalThis.Element };
  Object.defineProperty(globalThis, "localStorage", { configurable: true, value: dom.window.localStorage });
  Object.assign(globalThis, { document: dom.window.document, Element: dom.window.Element });

  let configured = false;
  let locked = true;
  let epoch = 0;
  const commands: string[] = [];
  const invoke: Invoke = async <T>(command: string) => {
    commands.push(command);
    if (command === "get_security_status") {
      return { configured, locked, epoch, retry_after_ms: 0, vault_mode: "legacy" } as T;
    }
    if (command === "initialize_security") {
      configured = true;
      locked = false;
      epoch = 1;
      return { configured, locked, epoch, retry_after_ms: 0, vault_mode: "legacy" } as T;
    }
    if (command === "unlock_security") {
      locked = false;
      epoch += 1;
      return { configured, locked, epoch, retry_after_ms: 0, vault_mode: "legacy" } as T;
    }
    if (command === "lock_security") {
      locked = true;
      epoch += 1;
      return { configured, locked, epoch, retry_after_ms: 0, vault_mode: "legacy" } as T;
    }
    if (command === "list_conversations" || command === "list_local_mcp_clients") return [] as T;
    if (command === "get_setup") {
      return {
        platform: "windows",
        archive_path: "fixture-archive",
        extension_path: "fixture-extension",
        native_host_name: "com.mindplace.capture",
        host_path: "fixture-host",
        extension_id: "fixture-id",
        registered_browsers: [],
        legacy_available: false,
      } as T;
    }
    if (command === "refresh_archive_index") return {} as T;
    return { running: false } as T;
  };

  try {
    await mountApplication({ root, invoke, clipboard: { writeText: async () => {} } });
    assert.ok(root.querySelector("#security-secret-confirm"), "fresh install asks for secret confirmation");
    root.querySelector<HTMLInputElement>("#security-secret")!.value = "correct horse battery";
    root.querySelector<HTMLInputElement>("#security-secret-confirm")!.value = "correct horse battery";
    root.querySelector<HTMLFormElement>("#security-form")!.dispatchEvent(
      new dom.window.Event("submit", { bubbles: true, cancelable: true }),
    );
    for (let i = 0; i < 8; i++) await delay(5);

    assert.ok(root.querySelector(".onboarding-workspace"), "first-run onboarding wizard opens in classic flow even with v2 flag enabled");
    assert.equal(root.querySelector(".ui2-shell"), null, "v2 shell must not bypass onboarding or mount simultaneously");
    assert.equal(dom.window.localStorage.getItem(UI2_SHELL_FEATURE_FLAG), "enabled", "preview opt-in stays intact");

    // Lock from classic onboarding/reader and verify gate returns without leaking wizard or preview.
    root.querySelector<HTMLButtonElement>("[data-action='lock']")!.click();
    await delay(20);
    assert.ok(root.querySelector("#security-form"), "gate returns after confirmed Lock");
    assert.equal(root.querySelector<HTMLElement>("#security-confirm-row")?.hidden, true, "configured profile now shows unlock, not setup");
    assert.equal(root.querySelector(".onboarding-workspace"), null);
    assert.equal(root.querySelector(".ui2-shell"), null);

    // Subsequent unlock opens v2 Shell preview, switches EN -> ID, and restores lang on Lock.
    root.querySelector<HTMLInputElement>("#security-secret")!.value = "correct horse battery";
    root.querySelector<HTMLFormElement>("#security-form")!.dispatchEvent(
      new dom.window.Event("submit", { bubbles: true, cancelable: true }),
    );
    await delay(50);
    assert.ok(root.querySelector(".ui2-shell"), "subsequent unlock enters flagged v2 shell");
    assert.equal(root.querySelector(".onboarding-workspace"), null);

    root.querySelector<HTMLButtonElement>('[data-ui2-seg-value="id"]')!.click();
    assert.equal(dom.window.document.documentElement.lang, "id");
    assert.equal(root.querySelector('[data-ui2-route="/home"]')?.getAttribute("aria-label"), "Beranda");

    root.querySelector<HTMLButtonElement>('[data-ui2-control="lock"]')!.click();
    await delay(20);
    assert.ok(root.querySelector("#security-form"), "v2 Lock returns to native gate");
    assert.equal(root.querySelector(".ui2-shell"), null);
    assert.equal(dom.window.document.documentElement.lang, "en", "Lock restores original gate document language");

    // Third unlock keeps persisted ID locale in v2 shell.
    root.querySelector<HTMLInputElement>("#security-secret")!.value = "correct horse battery";
    root.querySelector<HTMLFormElement>("#security-form")!.dispatchEvent(
      new dom.window.Event("submit", { bubbles: true, cancelable: true }),
    );
    await delay(50);
    assert.ok(root.querySelector(".ui2-shell"));
    assert.equal(dom.window.document.documentElement.lang, "id");
    assert.equal(root.querySelector('[data-ui2-route="/archive"]')?.getAttribute("aria-label"), "Arsip");
    assert.ok(commands.includes("initialize_security"));
    assert.ok(commands.includes("unlock_security"));
    assert.ok(commands.includes("lock_security"));
  } finally {
    Object.assign(globalThis, previousGlobals);
    if (originalStorage) Object.defineProperty(globalThis, "localStorage", originalStorage);
    else delete (globalThis as { localStorage?: Storage }).localStorage;
    dom.window.close();
  }
});

test("MP-UI2-04: Ctrl+K shortcut uses approved high-contrast token >= 4.5:1 in both light and dark themes", () => {
  const repoRoot = path.resolve(import.meta.dirname, "..");
  const tokensCss = fs.readFileSync(path.join(repoRoot, "src/ui2/ui-kit/tokens.css"), "utf8");
  const shellCss = fs.readFileSync(path.join(repoRoot, "src/ui2/shell/shell.css"), "utf8");

  const shortcutRule = /\.ui2-shell-shortcut\s*\{([^}]*)\}/.exec(shellCss)?.[1] ?? "";
  const colorTokenMatch = /color:\s*var\((--[a-z0-9-]+)\)/.exec(shortcutRule);
  assert.ok(colorTokenMatch, "shortcut color must use a UI2 CSS token");
  const fgToken = colorTokenMatch[1];
  assert.notEqual(fgToken, "--fg-3", "shortcut must not use low-contrast --fg-3");

  const lightBlock = /:root,\s*:root\[data-theme="light"\]\s*\{([^}]*)\}/.exec(tokensCss)?.[1] ?? "";
  const darkBlock = /:root\[data-theme="dark"\]\s*\{([^}]*)\}/.exec(tokensCss)?.[1] ?? "";
  assert.ok(lightBlock && darkBlock, "both light and dark token blocks must exist");

  const readHex = (block: string, token: string) => {
    const m = new RegExp(`${token}:\\s*(#[0-9a-fA-F]{6})\\s*;`).exec(block);
    assert.ok(m, `missing hex token ${token}`);
    return m[1];
  };
  const hexToLum = (hex: string) => {
    const channel = (start: number) => {
      const c = Number.parseInt(hex.slice(start, start + 2), 16) / 255;
      return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    };
    return 0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5);
  };
  const contrast = (hexA: string, hexB: string) => {
    const l1 = hexToLum(hexA);
    const l2 = hexToLum(hexB);
    return (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05);
  };

  for (const [theme, block] of [["light", lightBlock], ["dark", darkBlock]] as const) {
    const fg = readHex(block, fgToken);
    for (const bgToken of ["--surface", "--sunken"]) {
      const bg = readHex(block, bgToken);
      const ratio = contrast(fg, bg);
      assert.ok(ratio >= 4.5, `${theme} ${fgToken} (${fg}) on ${bgToken} (${bg}) contrast ${ratio.toFixed(2)} < 4.5`);
    }
  }
});

import assert from "node:assert/strict";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import { UI_ROUTES, UI_MODULES, createUiModuleRegistry, routeForWorkspace, type UiRouteId } from "../src/ui-contracts.js";
import { createUiButton, createUiCard, createUiDialog, createUiDrawer, createUiStatus, createUiTable, createUiStatePanel } from "../src/ui-primitives.js";
import { APPLICATION_SHELL_MARKUP, mountUiShell } from "../src/ui-shell.js";
import { createWorkspaceNavigation, setActiveWorkspace } from "../src/knowledge-os.js";

test("the route registry defines eight primary destinations and maps legacy workspaces", () => {
  assert.equal(UI_ROUTES.length, 8);
  assert.equal(new Set(UI_ROUTES.map((route) => route.id)).size, 8);
  assert.deepEqual(UI_ROUTES.map((route) => route.label), [
    "Overview", "Conversations", "Curated Memory", "Hive Mind Graph",
    "Files & Secure Vault", "AI Providers & MCP", "Setup & Onboarding", "Settings",
  ]);
  assert.equal(routeForWorkspace("library"), "overview");
  assert.equal(routeForWorkspace("people"), "curated-memory");
  assert.equal(routeForWorkspace("health"), "settings");
  assert.equal(routeForWorkspace("settings"), "settings");
  assert.equal(routeForWorkspace("graph"), "hive-mind-graph");
});

test("module registry rejects duplicate ownership and resolves route registrations", () => {
  const registry = createUiModuleRegistry(UI_MODULES);
  assert.equal(registry.list().length, 8);
  assert.equal(registry.forRoute("curated-memory")?.id, "curated-memory");
  assert.equal(registry.get("hive-mind-graph")?.route, "hive-mind-graph");
  assert.throws(() => createUiModuleRegistry([
    { id: "one", route: "overview", title: "One", availability: "available" },
    { id: "two", route: "overview", title: "Two", availability: "available" },
  ]), /MP_UI_MODULE_DUPLICATE_ROUTE/);
  assert.throws(() => createUiModuleRegistry([
    { id: "one", route: "overview", title: "One", availability: "available" },
    { id: "one", route: "conversations", title: "Two", availability: "available" },
  ]), /MP_UI_MODULE_DUPLICATE_ID/);
});

test("shared primitives keep caller strings inert and expose semantic states", () => {
  const document = new JSDOM("<!doctype html><html><body></body></html>").window.document;
  const button = createUiButton(document, "<img src=x onerror=alert(1)>", { variant: "primary" });
  assert.equal(button.textContent, "<img src=x onerror=alert(1)>");
  assert.equal(button.querySelector("img"), null);
  assert.equal(button.type, "button");
  const card = createUiCard(document, { title: "Archive", description: "Local capture summary." });
  assert.equal(card.element.querySelector("h2")?.textContent, "Archive");
  assert.equal(card.element.querySelector(".ui-card-description")?.textContent, "Local capture summary.");
  const status = createUiStatus(document, "Vault locked", "locked");
  assert.equal(status.dataset.state, "locked");
  assert.equal(status.getAttribute("role"), "status");
  const table = createUiTable(document, { caption: "Recent items", columns: ["Name", "State"], rows: [["One", status]] });
  assert.equal(table.querySelector("caption")?.textContent, "Recent items");
  assert.equal(table.querySelectorAll("tbody tr").length, 1);
  assert.equal(table.querySelector<HTMLElement>("tbody .ui-status")?.dataset.state, "locked");
  const empty = createUiTable(document, { caption: "Empty", columns: ["Name"], rows: [], emptyLabel: "No items." });
  assert.equal(empty.querySelector(".ui-table-empty")?.textContent, "No items.");
  const dialog = createUiDialog(document, { id: "test-dialog", title: "Details" });
  const drawer = createUiDrawer(document, { id: "test-drawer", title: "Inspector", placement: "left" });
  assert.equal(dialog.getAttribute("aria-labelledby"), "test-dialog-title");
  assert.equal(drawer.dataset.placement, "left");
  assert.ok(drawer.classList.contains("ui-drawer"));
  const state = createUiStatePanel(document, { state: "permission-denied", title: "Access denied", detail: "Grant access in settings." });
  assert.equal(state.getAttribute("role"), "alert");
  assert.equal(state.dataset.state, "permission-denied");
});

test("navigation keeps one current route and marks secondary lenses separately", () => {
  const document = new JSDOM("<!doctype html><html><body></body></html>").window.document;
  const nav = createWorkspaceNavigation(document);
  setActiveWorkspace(nav, "topics");
  assert.equal(nav.querySelectorAll('[aria-current="page"]').length, 1);
  assert.equal(nav.querySelector('[data-route="curated-memory"]')?.getAttribute("aria-current"), "page");
  assert.equal(nav.querySelector('[data-workspace="topics"]')?.getAttribute("aria-pressed"), "true");
  assert.equal(nav.querySelector('[data-subnav-for="curated-memory"]')?.hasAttribute("hidden"), false);
  assert.equal(nav.querySelector('[data-subnav-for="overview"]')?.hasAttribute("hidden"), true);
});

test("shell preferences, command palette and shortcut are accessible and disposable", () => {
  const dom = new JSDOM("<!doctype html><html><head><meta name='theme-color' content='#F4F4F5'></head><body><div id='app'></div></body></html>", { url: "https://shell.test/" });
  const root = dom.window.document.querySelector<HTMLElement>("#app")!;
  root.innerHTML = APPLICATION_SHELL_MARKUP;
  const nav = createWorkspaceNavigation(dom.window.document);
  root.querySelector("#knowledge-os-nav-slot")?.replaceWith(nav);
  let selected: UiRouteId | undefined;
  let selectedCommand: string | undefined;
  const shell = mountUiShell(root, {
    onNavigate: (route) => { selected = route; },
    onCommand: (command) => { selectedCommand = command.id; },
  });
  assert.deepEqual(shell.getPreferences(), { theme: "light", density: "comfortable", technicalMode: false, sidebarCollapsed: false });
  root.querySelector<HTMLButtonElement>("[data-action='toggle-theme']")!.click();
  root.querySelector<HTMLButtonElement>("[data-action='toggle-density']")!.click();
  root.querySelector<HTMLButtonElement>("[data-action='toggle-technical']")!.click();
  root.querySelector<HTMLButtonElement>("[data-action='toggle-sidebar']")!.click();
  assert.deepEqual(shell.getPreferences(), { theme: "dark", density: "compact", technicalMode: true, sidebarCollapsed: true });
  assert.equal(dom.window.document.documentElement.dataset.theme, "dark");
  assert.equal(dom.window.document.querySelector("meta[name='theme-color']")?.getAttribute("content"), "#09090B");
  assert.equal(root.querySelector(".app-shell")?.getAttribute("data-sidebar-collapsed"), "true");
  assert.equal(root.querySelector("[data-action='toggle-sidebar']")?.getAttribute("aria-expanded"), "false");
  assert.equal(root.querySelector('[data-subnav-for="overview"]')?.hasAttribute("hidden"), true);
  root.querySelector<HTMLButtonElement>("[data-action='toggle-sidebar']")!.click();
  assert.equal(root.querySelector('[data-subnav-for="overview"]')?.hasAttribute("hidden"), false);
  root.querySelector<HTMLButtonElement>("[data-action='toggle-sidebar']")!.click();
  root.querySelector<HTMLButtonElement>("[data-action='open-command-palette']")!.click();
  const palette = root.querySelector<HTMLDialogElement>("#command-palette")!;
  assert.equal(palette.hasAttribute("open"), true);
  const input = root.querySelector<HTMLInputElement>("#command-palette-search")!;
  input.value = "Hive";
  input.dispatchEvent(new dom.window.Event("input", { bubbles: true }));
  const option = root.querySelector<HTMLButtonElement>("[data-action='command-palette-navigate'][data-route='hive-mind-graph']");
  assert.ok(option);
  option!.click();
  assert.equal(selected, "hive-mind-graph");
  assert.equal(palette.hasAttribute("open"), false);
  root.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "k", ctrlKey: true, bubbles: true, cancelable: true }));
  assert.equal(palette.hasAttribute("open"), true);
  input.value = "People";
  input.dispatchEvent(new dom.window.Event("input", { bubbles: true }));
  root.querySelector<HTMLButtonElement>("[data-action='command-palette-command'][data-command-id='people']")!.click();
  assert.equal(selectedCommand, "people");
  assert.equal(palette.hasAttribute("open"), false);

  root.querySelector<HTMLButtonElement>("[data-action='open-command-palette']")!.click();
  input.value = "search memories";
  input.dispatchEvent(new dom.window.Event("input", { bubbles: true }));
  root.querySelector<HTMLButtonElement>("[data-action='command-palette-command'][data-command-id='search-all-sources']")!.click();
  assert.equal(selectedCommand, "search-all-sources");

  const unregister = shell.registerCommand({
    id: "search-memory",
    label: "Search memories",
    description: "Search memory statements and evidence.",
    keywords: ["memory", "search", "evidence"],
    action: "open-memory-search",
  });
  root.querySelector<HTMLButtonElement>("[data-action='open-command-palette']")!.click();
  input.value = "evidence";
  input.dispatchEvent(new dom.window.Event("input", { bubbles: true }));
  root.querySelector<HTMLButtonElement>("[data-action='command-palette-command'][data-command-id='search-memory']")!.click();
  assert.equal(selectedCommand, "search-memory");
  unregister();
  assert.throws(() => shell.registerCommand({
    id: "bad-command", label: "Bad", description: "Missing target.",
  }), /MP_UI_COMMAND_INVALID/);
  shell.setActiveRoute("settings");
  assert.equal(root.querySelector("#route-breadcrumb")?.textContent, "Settings");
  shell.destroy();
  assert.equal(root.querySelector("#command-palette"), null);
});

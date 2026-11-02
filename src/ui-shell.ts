import { UI_ROUTES, getUiRoute, isUiRouteId, type UiRouteId } from "./ui-contracts.js";
import { createUiButton, createUiDialog } from "./ui-primitives.js";

export const APPLICATION_SHELL_MARKUP = "\n  <a class=\"skip-link\" href=\"#reader-view\">Skip to main content</a>\n  <div class=\"app-shell\" data-active-route=\"overview\" data-sidebar-collapsed=\"false\">\n    <aside class=\"library app-sidebar\" id=\"app-sidebar\" aria-label=\"Main navigation\">\n      <div class=\"brand-lockup\"><span class=\"brand-mark\" aria-hidden=\"true\">m</span><span class=\"brand-name\">Mind Place</span></div>\n      <div id=\"knowledge-os-nav-slot\"></div>\n      <section class=\"sidebar-context\" data-route-context=\"conversations\" aria-label=\"Conversation archive\">\n        <div class=\"library-heading-row\"><h2>Conversations</h2><button class=\"quiet-button refresh-button\" type=\"button\" data-action=\"refresh\" aria-label=\"Refresh archive\">Refresh</button></div>\n        <label class=\"search-label\" for=\"archive-search\">Search conversations</label>\n        <div class=\"search-field\"><svg aria-hidden=\"true\" viewBox=\"0 0 20 20\"><circle cx=\"8.8\" cy=\"8.8\" r=\"5.6\"></circle><path d=\"m13 13 4 4\"></path></svg><input id=\"archive-search\" type=\"search\" autocomplete=\"off\" placeholder=\"Search titles and messages\" /><kbd aria-hidden=\"true\">/</kbd></div>\n        <div class=\"list-heading\">Saved conversations</div>\n        <div id=\"conversation-list\" class=\"conversation-list\" role=\"list\" aria-label=\"Saved conversations\" aria-live=\"polite\"><p class=\"list-note\">Loading archive…</p></div>\n      </section>\n      <footer class=\"library-footer sidebar-footer\">\n        <div class=\"offline-note\"><span class=\"offline-dot\" aria-hidden=\"true\"></span><span>Stored on this device</span></div>\n        <button class=\"setup-link\" type=\"button\" data-action=\"open-library-workspace\" data-workspace=\"library\">Search all sources</button>\n        <button class=\"setup-link\" type=\"button\" data-action=\"open-health-inspector\" data-workspace=\"health\">Operational diagnostics</button>\n      </footer>\n    </aside><main class=\"reader-main app-main\"><header class=\"app-topbar\">\n        <div class=\"app-breadcrumb\" aria-label=\"Breadcrumb\"><span class=\"breadcrumb-root\">Mind Place</span><span class=\"breadcrumb-divider\" aria-hidden=\"true\">/</span><span id=\"route-breadcrumb\">Overview</span></div>\n        <div class=\"topbar-actions\">\n          <button class=\"quiet-button command-palette-trigger\" type=\"button\" data-action=\"open-command-palette\" aria-haspopup=\"dialog\" aria-controls=\"command-palette\">Search or jump <kbd>Ctrl K</kbd></button>\n          <button class=\"quiet-button\" type=\"button\" data-action=\"toggle-theme\" aria-pressed=\"false\">Theme: Light</button>\n          <button class=\"quiet-button\" type=\"button\" data-action=\"toggle-density\" aria-pressed=\"false\">Density: Comfortable</button>\n          <button class=\"quiet-button\" type=\"button\" data-action=\"toggle-technical\" aria-pressed=\"false\">Technical: Off</button>\n          <button class=\"icon-button sidebar-toggle\" type=\"button\" data-action=\"toggle-sidebar\" aria-controls=\"app-sidebar\" aria-expanded=\"true\" aria-label=\"Collapse sidebar\">Menu</button>\n        </div>\n      </header>\n      <header class=\"reader-toolbar\">\n        <div class=\"toolbar-context\"><span class=\"context-mark\" aria-hidden=\"true\"></span><span>Overview</span></div>\n        <div class=\"toolbar-actions\">\n          <button class=\"quiet-button\" type=\"button\" data-action=\"lock\">Lock</button>\n          <span id=\"action-status\" class=\"action-status\" role=\"status\" aria-live=\"polite\"></span>\n          <button class=\"quiet-button\" type=\"button\" data-action=\"copy-answer\" disabled>Copy answer</button>\n          <button class=\"primary-button\" type=\"button\" data-action=\"export-markdown\" disabled><svg aria-hidden=\"true\" viewBox=\"0 0 20 20\"><path d=\"M10 2.75v9.5m0 0 3.25-3.25M10 12.25 6.75 9M3.5 13.75v2.5c0 .55.45 1 1 1h11c.55 0 1-.45 1-1v-2.5\"></path></svg><span>Export Markdown</span></button>\n        </div>\n      </header><section id=\"reader-view\" class=\"reader-view\" tabindex=\"-1\" aria-live=\"polite\" aria-busy=\"true\">\n        <div class=\"state-panel loading-panel\"><span class=\"loading-mark\" aria-hidden=\"true\"></span><p>Opening your archive…</p></div>\n      </section>\n    </main>\n  </div>\n<dialog id=\"setup-dialog\" class=\"setup-dialog\" aria-labelledby=\"setup-title\">\n    <div class=\"dialog-header\">\n      <div><p class=\"eyebrow\">Mind Place / Preferences</p><h2 id=\"setup-title\">Settings</h2><p class=\"settings-subtitle\">Manage your archive, documents and connections.</p></div>\n      <button class=\"icon-button\" type=\"button\" data-action=\"close-setup\" aria-label=\"Close setup\">×</button>\n    </div>\n    <div class=\"settings-layout\">\n      <nav class=\"settings-nav\" role=\"tablist\" aria-label=\"Settings categories\" aria-orientation=\"vertical\">\n        <button class=\"settings-tab\" type=\"button\" role=\"tab\" id=\"settings-tab-archive\" aria-controls=\"settings-panel-archive\" aria-selected=\"true\" tabindex=\"0\" data-settings-tab=\"archive\"><span>01</span>Archive &amp; Capture</button>\n        <button class=\"settings-tab\" type=\"button\" role=\"tab\" id=\"settings-tab-documents\" aria-controls=\"settings-panel-documents\" aria-selected=\"false\" tabindex=\"-1\" data-settings-tab=\"documents\"><span>02</span>Documents &amp; Permissions</button>\n        <button class=\"settings-tab\" type=\"button\" role=\"tab\" id=\"settings-tab-connections\" aria-controls=\"settings-panel-connections\" aria-selected=\"false\" tabindex=\"-1\" data-settings-tab=\"connections\"><span>03</span>Connections &amp; MCP</button>\n        <button class=\"settings-tab\" type=\"button\" role=\"tab\" id=\"settings-tab-privacy\" aria-controls=\"settings-panel-privacy\" aria-selected=\"false\" tabindex=\"-1\" data-settings-tab=\"privacy\"><span>04</span>Privacy &amp; Security</button>\n      </nav>\n      <div id=\"setup-content\" class=\"setup-content\" aria-live=\"off\">\n        <p class=\"list-note\">Loading settings…</p>\n      </div>\n    </div>\n    <div class=\"dialog-footer\"><button class=\"quiet-button\" type=\"button\" data-action=\"lock\">Lock</button><button class=\"quiet-button\" type=\"button\" data-action=\"close-setup\">Done</button></div>\n  </dialog>\n";

export interface UiShellPreferences {
  readonly theme: "light" | "dark";
  readonly density: "comfortable" | "compact";
  readonly technicalMode: boolean;
  readonly sidebarCollapsed: boolean;
}
export interface UiShellController {
  setActiveRoute(route: UiRouteId): void;
  getPreferences(): UiShellPreferences;
  destroy(): void;
}
export interface UiShellOptions { readonly onNavigate: (route: UiRouteId, trigger: HTMLElement) => void; }

function showDialog(dialog: HTMLDialogElement): void {
  if (typeof dialog.showModal === "function") {
    try { dialog.showModal(); return; } catch { /* Embedded webviews and test DOMs may omit modal support. */ }
  }
  dialog.setAttribute("open", "");
}
function hideDialog(dialog: HTMLDialogElement): void {
  if (typeof dialog.close === "function" && dialog.open) {
    try { dialog.close(); return; } catch { /* Remove the open attribute below. */ }
  }
  dialog.removeAttribute("open");
}

export function mountUiShell(root: HTMLElement, options: UiShellOptions): UiShellController {
  const document = root.ownerDocument;
  const shell = root.querySelector<HTMLElement>(".app-shell");
  if (!shell) throw new Error("MP_UI_SHELL_MISSING");
  const breadcrumb = root.querySelector<HTMLElement>("#route-breadcrumb");
  const themeButton = root.querySelector<HTMLButtonElement>("[data-action='toggle-theme']");
  const densityButton = root.querySelector<HTMLButtonElement>("[data-action='toggle-density']");
  const technicalButton = root.querySelector<HTMLButtonElement>("[data-action='toggle-technical']");
  const sidebarButton = root.querySelector<HTMLButtonElement>("[data-action='toggle-sidebar']");
  if (!themeButton || !densityButton || !technicalButton || !sidebarButton) throw new Error("MP_UI_SHELL_CONTROLS_MISSING");

  const palette = createUiDialog(document, {
    id: "command-palette", title: "Search or jump",
    description: "Find an available Mind Place destination.", className: "ui-command-palette",
  });
  const paletteBody = palette.querySelector<HTMLElement>(".ui-dialog-body");
  if (!paletteBody) throw new Error("MP_UI_COMMAND_PALETTE_BODY_MISSING");
  const search = document.createElement("input");
  search.id = "command-palette-search";
  search.type = "search";
  search.autocomplete = "off";
  search.placeholder = "Search destinations";
  search.setAttribute("aria-label", "Search destinations");
  search.setAttribute("aria-controls", "command-palette-results");
  const results = document.createElement("div");
  results.id = "command-palette-results";
  results.className = "ui-command-results";
  results.setAttribute("role", "listbox");
  results.setAttribute("aria-label", "Available destinations");
  paletteBody.append(search, results);
  root.append(palette);

  const html = document.documentElement;
  let theme: "light" | "dark" = html.dataset.theme === "dark" ? "dark" : "light";
  let density: "comfortable" | "compact" = html.dataset.density === "compact" ? "compact" : "comfortable";
  let technicalMode = html.dataset.technicalMode === "true";
  let sidebarCollapsed = shell.dataset.sidebarCollapsed === "true";

  const renderResults = (query: string): void => {
    const normalized = query.trim().toLocaleLowerCase();
    const matches = UI_ROUTES.filter((route) =>
      !normalized || (route.label + " " + route.description + " " + route.group).toLocaleLowerCase().includes(normalized),
    );
    results.replaceChildren();
    for (const route of matches) {
      const row = document.createElement("div");
      row.className = "ui-command-result";
      const button = createUiButton(document, route.label, { variant: "quiet", className: "ui-command-option" });
      button.dataset.action = "command-palette-navigate";
      button.dataset.route = route.id;
      button.setAttribute("role", "option");
      button.setAttribute("aria-selected", "false");
      const description = document.createElement("span");
      description.className = "ui-command-description";
      description.textContent = route.group + " · " + route.description;
      row.append(button, description);
      results.append(row);
    }
    if (matches.length === 0) {
      const empty = document.createElement("p");
      empty.className = "ui-command-empty";
      empty.textContent = "No matching destinations.";
      results.append(empty);
    }
  };
  const syncSecondaryNavigation = (): void => {
    for (const group of root.querySelectorAll<HTMLElement>("[data-subnav-for]")) {
      group.hidden = sidebarCollapsed || group.dataset.subnavFor !== shell.dataset.activeRoute;
    }
  };
  const applyPreferences = (): void => {
    html.dataset.theme = theme;
    html.dataset.density = density;
    html.dataset.technicalMode = String(technicalMode);
    const themeColor = document.querySelector<HTMLMetaElement>('meta[name="theme-color"]');
    if (themeColor) themeColor.content = theme === "dark" ? "#09090B" : "#F4F4F5";
    shell.dataset.sidebarCollapsed = String(sidebarCollapsed);
    themeButton.textContent = "Theme: " + (theme === "light" ? "Light" : "Dark");
    themeButton.setAttribute("aria-pressed", String(theme === "dark"));
    themeButton.setAttribute("aria-label", "Switch to " + (theme === "light" ? "dark" : "light") + " theme");
    densityButton.textContent = "Density: " + (density === "comfortable" ? "Comfortable" : "Compact");
    densityButton.setAttribute("aria-pressed", String(density === "compact"));
    densityButton.setAttribute("aria-label", "Switch to " + (density === "comfortable" ? "compact" : "comfortable") + " density");
    technicalButton.textContent = "Technical: " + (technicalMode ? "On" : "Off");
    technicalButton.setAttribute("aria-pressed", String(technicalMode));
    technicalButton.setAttribute("aria-label", technicalMode ? "Hide technical details" : "Show technical details");
    sidebarButton.textContent = sidebarCollapsed ? "Expand" : "Collapse";
    sidebarButton.setAttribute("aria-expanded", String(!sidebarCollapsed));
    sidebarButton.setAttribute("aria-label", sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar");
    syncSecondaryNavigation();
  };
  const setActiveRoute = (route: UiRouteId): void => {
    shell.dataset.activeRoute = route;
    if (breadcrumb) breadcrumb.textContent = getUiRoute(route).label;
    document.title = getUiRoute(route).label + " — Mind Place";
    syncSecondaryNavigation();
  };
  const openPalette = (): void => {
    search.value = "";
    renderResults("");
    showDialog(palette);
    search.focus();
  };
  const closePalette = (): void => hideDialog(palette);
  const clickHandler = (event: Event): void => {
    const target = event.target;
    if (!(target instanceof document.defaultView!.Element)) return;
    const element = target.closest<HTMLElement>("[data-action]");
    if (!element) return;
    const action = element.dataset.action;
    if (action === "open-command-palette") openPalette();
    else if (action === "toggle-theme") { theme = theme === "light" ? "dark" : "light"; applyPreferences(); }
    else if (action === "toggle-density") { density = density === "comfortable" ? "compact" : "comfortable"; applyPreferences(); }
    else if (action === "toggle-technical") { technicalMode = !technicalMode; applyPreferences(); }
    else if (action === "toggle-sidebar") { sidebarCollapsed = !sidebarCollapsed; applyPreferences(); }
    else if (action === "close-ui-dialog" && element.dataset.dialogId === "command-palette") closePalette();
    else if (action === "command-palette-navigate") {
      const route = element.dataset.route;
      if (route && isUiRouteId(route)) { closePalette(); options.onNavigate(route, element); }
    }
  };
  const inputHandler = (): void => renderResults(search.value);
  const cancelHandler = (event: Event): void => { event.preventDefault(); closePalette(); };
  const keyHandler = (event: KeyboardEvent): void => {
    if ((event.ctrlKey || event.metaKey) && !event.altKey && !event.shiftKey && event.key.toLowerCase() === "k") {
      event.preventDefault(); openPalette(); return;
    }
    if (!palette.open) return;
    if (event.key === "Escape") { event.preventDefault(); closePalette(); return; }
    if (event.target === search && event.key === "ArrowDown") {
      const first = results.querySelector<HTMLButtonElement>("button[data-route]");
      if (first) { event.preventDefault(); first.focus(); }
    }
  };
  search.addEventListener("input", inputHandler);
  palette.addEventListener("cancel", cancelHandler);
  root.addEventListener("click", clickHandler);
  root.addEventListener("keydown", keyHandler);
  applyPreferences();
  setActiveRoute("overview");
  return {
    setActiveRoute,
    getPreferences: () => ({ theme, density, technicalMode, sidebarCollapsed }),
    destroy: () => {
      root.removeEventListener("click", clickHandler);
      root.removeEventListener("keydown", keyHandler);
      search.removeEventListener("input", inputHandler);
      palette.removeEventListener("cancel", cancelHandler);
      hideDialog(palette);
      palette.remove();
    },
  };
}

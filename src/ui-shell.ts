import { UI_ROUTES, getUiRoute, isUiRouteId, type UiCommandRegistration, type UiRouteId, type UiWorkspaceAlias } from "./ui-contracts.js";
import { createUiButton, createUiDialog } from "./ui-primitives.js";

export const APPLICATION_SHELL_MARKUP = `
  <a class="skip-link" href="#reader-view">Skip to main content</a>
  <div class="app-shell" data-active-route="overview" data-sidebar-collapsed="false">
    <aside class="library app-sidebar sidebar" id="app-sidebar" aria-label="Main navigation">
      <div class="brand-row">
        <div class="brand-lockup brand">
          <span class="brand-mark" aria-hidden="true">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <circle cx="12" cy="12" r="3"></circle>
              <path d="M12 2v4M12 18v4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M2 12h4M18 12h4M4.93 19.07l2.83-2.83M16.24 7.76l2.83-2.83"></path>
            </svg>
          </span>
          <div class="brand-text">
            <span class="brand-name">Mind Place</span>
            <span class="brand-sub">v2.4 · LOCAL-FIRST</span>
          </div>
        </div>
        <button class="collapse-btn icon-button sidebar-toggle" type="button" data-action="toggle-sidebar" aria-controls="app-sidebar" aria-expanded="true" aria-label="Collapse sidebar">☰</button>
      </div>
      <div class="vault-pill" role="region" aria-label="Vault session status">
        <div class="vault-left">
          <span class="vault-dot" aria-hidden="true"></span>
          <div class="vault-copy">
            <b>Local Vault Unlocked</b>
            <small id="sidebar-vault-sub">SQLite · Session Active</small>
          </div>
        </div>
        <button class="vault-lock-quick quiet-button" type="button" data-action="lock" title="Lock session">Lock</button>
      </div>
      <div id="knowledge-os-nav-slot"></div>
      <section class="sidebar-context" data-route-context="conversations" aria-label="Conversation archive" hidden>
        <div class="library-heading-row"><h2>Conversations</h2><button class="quiet-button refresh-button" type="button" data-action="refresh" aria-label="Refresh archive">Refresh</button></div>
        <label class="search-label" for="archive-search">Search conversations</label>
        <div class="search-field"><svg aria-hidden="true" viewBox="0 0 20 20"><circle cx="8.8" cy="8.8" r="5.6"></circle><path d="m13 13 4 4"></path></svg><input id="archive-search" type="search" autocomplete="off" placeholder="Search titles and messages" /><kbd aria-hidden="true">/</kbd></div>
        <div class="list-heading">Saved conversations</div>
        <div id="conversation-list" class="conversation-list" role="list" aria-label="Saved conversations" aria-live="polite"><p class="list-note">Loading archive…</p></div>
      </section>
      <footer class="library-footer sidebar-footer">
        <div class="mcp-status-bar">
          <div class="offline-note"><span class="offline-dot" aria-hidden="true"></span><span>MCP · LOOPBACK</span></div>
          <span class="tag mono" id="sidebar-mcp-badge">LOCAL</span>
        </div>
        <div class="sidebar-quick-links">
          <button class="setup-link" type="button" data-action="open-library-workspace" data-workspace="library">Search all sources</button>
          <button class="setup-link" type="button" data-action="open-health-inspector" data-workspace="health">Diagnostics</button>
        </div>
        <div class="profile-row">
          <div class="avatar" aria-hidden="true">MP</div>
          <div class="profile-copy">
            <b>Owner</b>
            <small>Local Authority Root</small>
          </div>
        </div>
      </footer>
    </aside>
    <main class="reader-main app-main main">
      <header class="app-topbar topbar">
        <div class="app-breadcrumb breadcrumb" aria-label="Breadcrumb">
          <span class="breadcrumb-root">Mind Place</span>
          <span class="breadcrumb-divider breadcrumb-sep" aria-hidden="true">/</span>
          <span id="route-breadcrumb">Overview</span>
        </div>
        <button class="quiet-button command-palette-trigger command-trigger" type="button" data-action="open-command-palette" aria-haspopup="dialog" aria-controls="command-palette">
          <span class="command-trigger-left">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
              <circle cx="11" cy="11" r="7"></circle>
              <path d="m20 20-3.5-3.5"></path>
            </svg>
            <span class="command-trigger-text">Search conversations, memories, files, or run command...</span>
          </span>
          <kbd class="kbd">Ctrl K</kbd>
        </button>
        <div class="topbar-actions top-actions">
          <button class="quiet-button pill-toggle" type="button" data-action="toggle-technical" aria-pressed="false">Technical: Off</button>
          <button class="quiet-button pill-toggle" type="button" data-action="toggle-density" aria-pressed="false">Density: Comfortable</button>
          <button class="quiet-button pill-toggle" type="button" data-action="toggle-theme" aria-pressed="false">Theme: Light</button>
          <div class="reader-toolbar">
            <div class="toolbar-context"><span class="context-mark" aria-hidden="true"></span><span>Overview</span></div>
            <div class="toolbar-actions">
              <span id="action-status" class="action-status" role="status" aria-live="polite"></span>
              <button class="quiet-button" type="button" data-action="copy-answer" disabled>Copy answer</button>
              <button class="primary-button" type="button" data-action="export-markdown" disabled><span>Export Markdown</span></button>
              <button class="quiet-button" type="button" data-action="lock">Lock</button>
            </div>
          </div>
        </div>
      </header>
      <section id="reader-view" class="reader-view content" tabindex="-1" aria-live="polite" aria-busy="true">
        <div class="state-panel loading-panel"><span class="loading-mark" aria-hidden="true"></span><p>Opening your archive…</p></div>
      </section>
    </main>
  </div>
  <dialog id="setup-dialog" class="setup-dialog" aria-labelledby="setup-title">
    <div class="dialog-header">
      <div><p class="eyebrow">Mind Place / Preferences</p><h2 id="setup-title">Settings</h2><p class="settings-subtitle">Manage your archive, documents and connections.</p></div>
      <button class="icon-button" type="button" data-action="close-setup" aria-label="Close setup">×</button>
    </div>
    <div class="settings-layout">
      <nav class="settings-nav" role="tablist" aria-label="Settings categories" aria-orientation="vertical">
        <button class="settings-tab" type="button" role="tab" id="settings-tab-archive" aria-controls="settings-panel-archive" aria-selected="true" tabindex="0" data-settings-tab="archive"><span>01</span>Archive &amp; Capture</button>
        <button class="settings-tab" type="button" role="tab" id="settings-tab-documents" aria-controls="settings-panel-documents" aria-selected="false" tabindex="-1" data-settings-tab="documents"><span>02</span>Documents &amp; Permissions</button>
        <button class="settings-tab" type="button" role="tab" id="settings-tab-connections" aria-controls="settings-panel-connections" aria-selected="false" tabindex="-1" data-settings-tab="connections"><span>03</span>Connections &amp; MCP</button>
        <button class="settings-tab" type="button" role="tab" id="settings-tab-privacy" aria-controls="settings-panel-privacy" aria-selected="false" tabindex="-1" data-settings-tab="privacy"><span>04</span>Privacy &amp; Security</button>
      </nav>
      <div id="setup-content" class="setup-content" aria-live="off">
        <p class="list-note">Loading settings…</p>
      </div>
    </div>
    <div class="dialog-footer"><button class="quiet-button" type="button" data-action="lock">Lock</button><button class="quiet-button" type="button" data-action="close-setup">Done</button></div>
  </dialog>
`;

export interface UiShellPreferences {
  readonly theme: "light" | "dark";
  readonly density: "comfortable" | "compact";
  readonly technicalMode: boolean;
  readonly sidebarCollapsed: boolean;
}
export interface UiShellController {
  setActiveRoute(route: UiRouteId, workspace?: UiWorkspaceAlias): void;
  registerCommand(command: UiCommandRegistration): () => void;
  getPreferences(): UiShellPreferences;
  destroy(): void;
}
export interface UiShellOptions {
  readonly onNavigate: (route: UiRouteId, trigger: HTMLElement) => void;
  readonly onCommand: (command: UiCommandRegistration, trigger: HTMLElement) => void;
}

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

  let lastFocusedElement: HTMLElement | null = null;
  let resultLimit = 15;
  let showAllResults = false;

  const commands = new Map<string, UiCommandRegistration>();
  const renderResults = (query: string): void => {
    const terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
    const matchesQuery = (value: string): boolean => {
      const haystack = value.toLocaleLowerCase();
      return terms.length === 0 || terms.every((term) => haystack.includes(term));
    };
    const allRoutes = UI_ROUTES.filter((route) =>
      matchesQuery(route.label + " " + route.description + " " + route.group),
    );
    const allActions = [...commands.values()].filter((command) =>
      matchesQuery(command.label + " " + command.description + " " + (command.keywords ?? []).join(" ")),
    );
    const totalMatches = allRoutes.length + allActions.length;
    const routes = showAllResults ? allRoutes : allRoutes.slice(0, resultLimit);
    const actions = showAllResults ? allActions : allActions.slice(0, resultLimit);
    results.replaceChildren();

    if (routes.length > 0) {
      const groupHeading = document.createElement("div");
      groupHeading.className = "ui-command-group-heading";
      groupHeading.setAttribute("role", "presentation");
      groupHeading.textContent = "Destinations";
      results.append(groupHeading);
    }
    for (const route of routes) {
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

    if (actions.length > 0) {
      const groupHeading = document.createElement("div");
      groupHeading.className = "ui-command-group-heading";
      groupHeading.setAttribute("role", "presentation");
      groupHeading.textContent = "Actions";
      results.append(groupHeading);
    }
    for (const command of actions) {
      const row = document.createElement("div");
      row.className = "ui-command-result";
      const button = createUiButton(document, command.label, { variant: "quiet", className: "ui-command-option" });
      button.dataset.action = "command-palette-command";
      button.dataset.commandId = command.id;
      button.setAttribute("role", "option");
      button.setAttribute("aria-selected", "false");
      const description = document.createElement("span");
      description.className = "ui-command-description";
      description.textContent = "Action · " + command.description;
      row.append(button, description);
      results.append(row);
    }

    if (!showAllResults && totalMatches > routes.length + actions.length) {
      const moreRow = document.createElement("div");
      moreRow.className = "ui-command-more-row";
      const moreButton = createUiButton(document, `Show all ${totalMatches} results`, {
        variant: "link",
        className: "ui-command-more-button",
      });
      moreButton.dataset.action = "command-palette-show-all";
      moreRow.append(moreButton);
      results.append(moreRow);
    }

    if (totalMatches === 0) {
      const empty = document.createElement("p");
      empty.className = "ui-command-empty";
      empty.textContent = "No matching destinations or actions.";
      results.append(empty);
    }
  };
  const registerCommand = (command: UiCommandRegistration): (() => void) => {
    const id = command.id.trim();
    const label = command.label.trim();
    const description = command.description.trim();
    const hasRoute = command.route !== undefined;
    const hasAction = Boolean(command.action?.trim());
    if (!id || id !== command.id || !label || !description || hasRoute === hasAction) {
      throw new Error("MP_UI_COMMAND_INVALID");
    }
    if (hasRoute && !isUiRouteId(command.route!)) throw new Error("MP_UI_COMMAND_UNKNOWN_ROUTE");
    if (commands.has(id)) throw new Error("MP_UI_COMMAND_DUPLICATE_ID");
    const frozen = Object.freeze({ ...command, keywords: Object.freeze([...(command.keywords ?? [])]) });
    commands.set(id, frozen);
    if (palette.open) renderResults(search.value);
    return () => {
      if (commands.delete(id) && palette.open) renderResults(search.value);
    };
  };
  const defaultCommands: readonly UiCommandRegistration[] = [
    { id: "search-all-sources", label: "Search all sources", description: "Search conversations, memories, folders and Hive.", keywords: ["search", "conversation", "memory", "folder", "hive", "source"], action: "open-library-workspace", workspace: "library" },
    { id: "projects", label: "Projects", description: "Open project memories.", keywords: ["project", "memory", "collection"], action: "open-entity-workspace", workspace: "projects" },
    { id: "people", label: "People", description: "Open people memories.", keywords: ["person", "people", "memory", "collection"], action: "open-entity-workspace", workspace: "people" },
    { id: "topics", label: "Topics", description: "Open topic memories.", keywords: ["topic", "memory", "collection"], action: "open-entity-workspace", workspace: "topics" },
    { id: "operational-diagnostics", label: "Operational diagnostics", description: "Inspect storage, retrieval and provider health.", keywords: ["health", "status", "recovery", "diagnostics"], action: "open-health-inspector", workspace: "health" },
  ];
  for (const command of defaultCommands) registerCommand(command);
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
    // The icon button is 30px wide: full words silently extend its scroll width past the viewport.
    // Use a compact glyph and preserve the explicit, stateful accessible name below.
    sidebarButton.textContent = "☰";
    sidebarButton.setAttribute("aria-expanded", String(!sidebarCollapsed));
    sidebarButton.setAttribute("aria-label", sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar");
    syncSecondaryNavigation();
  };
  const setActiveRoute = (route: UiRouteId, workspace?: UiWorkspaceAlias): void => {
    shell.dataset.activeRoute = route;
    const secondaryLabel: Partial<Record<UiWorkspaceAlias, string>> = {
      library: "Library search", projects: "Projects", people: "People", topics: "Topics", health: "Diagnostics",
    };
    const detail = workspace ? secondaryLabel[workspace] : undefined;
    const visibleTitle = getUiRoute(route).label + (detail ? " / " + detail : "");
    if (breadcrumb) breadcrumb.textContent = visibleTitle;
    document.title = visibleTitle + " — Mind Place";
    syncSecondaryNavigation();
  };
  const openPalette = (trigger?: HTMLElement): void => {
    lastFocusedElement = trigger ?? (document.activeElement as HTMLElement | null);
    search.value = "";
    showAllResults = false;
    renderResults("");
    showDialog(palette);
    search.focus();
  };
  const closePalette = (): void => {
    hideDialog(palette);
    if (lastFocusedElement && typeof lastFocusedElement.focus === "function") {
      try {
        lastFocusedElement.focus();
      } catch {
        // Safe fallback in test DOMs
      }
    }
  };
  const clickHandler = (event: Event): void => {
    const target = event.target;
    if (!(target instanceof document.defaultView!.Element)) return;
    const element = target.closest<HTMLElement>("[data-action]");
    if (!element) return;
    const action = element.dataset.action;
    if (action === "open-command-palette") openPalette(element);
    else if (action === "toggle-theme") { theme = theme === "light" ? "dark" : "light"; applyPreferences(); }
    else if (action === "toggle-density") { density = density === "comfortable" ? "compact" : "comfortable"; applyPreferences(); }
    else if (action === "toggle-technical") { technicalMode = !technicalMode; applyPreferences(); }
    else if (action === "toggle-sidebar") { sidebarCollapsed = !sidebarCollapsed; applyPreferences(); }
    else if (action === "close-ui-dialog" && element.dataset.dialogId === "command-palette") closePalette();
    else if (action === "command-palette-show-all") {
      showAllResults = true;
      renderResults(search.value);
    }
    else if (action === "command-palette-navigate") {
      const route = element.dataset.route;
      if (route && isUiRouteId(route)) { closePalette(); options.onNavigate(route, element); }
    } else if (action === "command-palette-command") {
      const command = commands.get(element.dataset.commandId ?? "");
      if (command) { closePalette(); options.onCommand(command, element); }
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
    const optionsList = Array.from(results.querySelectorAll<HTMLButtonElement>(".ui-command-option"));
    if (event.target === search && event.key === "ArrowDown") {
      const first = optionsList[0];
      if (first) { event.preventDefault(); first.focus(); }
    } else if (event.target instanceof document.defaultView!.HTMLButtonElement && event.target.classList.contains("ui-command-option")) {
      const currentIndex = optionsList.indexOf(event.target);
      if (currentIndex !== -1) {
        if (event.key === "ArrowDown") {
          event.preventDefault();
          const next = optionsList[currentIndex + 1] ?? optionsList[0];
          next?.focus();
        } else if (event.key === "ArrowUp") {
          event.preventDefault();
          if (currentIndex === 0) {
            search.focus();
          } else {
            const prev = optionsList[currentIndex - 1];
            prev?.focus();
          }
        }
      }
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
    registerCommand,
    getPreferences: () => ({ theme, density, technicalMode, sidebarCollapsed }),
    destroy: () => {
      root.removeEventListener("click", clickHandler);
      root.removeEventListener("keydown", keyHandler);
      search.removeEventListener("input", inputHandler);
      palette.removeEventListener("cancel", cancelHandler);
      hideDialog(palette);
      commands.clear();
      palette.remove();
    },
  };
}

import "../ui-kit/tokens.css";
import "../ui-kit/primitives.css";
import "./shell.css";
import type { Invoke } from "../../reader.js";
import { createI18nController, type Ui2I18nController, type Ui2TestLocale } from "../i18n/index.js";
import {
  createUi2Breadcrumb, createUi2Button, createUi2Dialog, createUi2EmptyState,
  createUi2Icon, createUi2Segmented, ensureUi2IconSprite,
} from "../ui-kit/index.js";
import { UI2_SHELL_DESTINATIONS, readUi2Hash, ui2HashForPath, resolveUi2Route, type Ui2ResolvedRoute } from "./routes.js";

export interface Ui2ShellOptions {
  onLock(): void | Promise<void>;
  onSwitchClassic?(): void;
  i18n?: Ui2I18nController;
}

export interface Ui2ShellHandle {
  readonly i18n: Ui2I18nController;
  getRoute(): Ui2ResolvedRoute;
  navigate(path: string): void;
  destroy(): void;
}

export const UI2_SESSION_EXPIRED = "MP_UI_SESSION_EXPIRED";

/** Both initiation and completion must belong to the same native unlock epoch. */
export function fenceUi2EpochInvoke(invoke: Invoke, isCurrent: () => boolean): Invoke {
  return async <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
    if (!isCurrent()) throw new Error(UI2_SESSION_EXPIRED);
    const result = await invoke<T>(command, args);
    if (!isCurrent()) throw new Error(UI2_SESSION_EXPIRED);
    return result;
  };
}

export function mountUi2Shell(root: HTMLElement, options: Ui2ShellOptions): Ui2ShellHandle {
  const doc = root.ownerDocument;
  const win = doc.defaultView;
  if (!win) throw new Error("MP_UI2_DOCUMENT_WINDOW_MISSING");
  ensureUi2IconSprite(doc);
  let storage: Storage | undefined;
  try { storage = win.localStorage; } catch { storage = undefined; }
  const i18n = options.i18n ?? createI18nController({ storage });
  const shell = doc.createElement("div");
  shell.className = "ui2-root ui2-shell";
  const routeAtMount = readUi2Hash(win.location.hash);
  let route = routeAtMount;
  let collapsed = false;
  let mobileOpen = false;
  let disposed = false;
  let searchDialog: ReturnType<typeof createUi2Dialog> | undefined;

  const isNarrow = () => win.matchMedia?.("(max-width: 780px)").matches ?? false;

  function focusAfterRender(selector: string): void {
    shell.querySelector<HTMLElement>(selector)?.focus();
  }

  function closeMobileNavigation(restoreFocus: boolean): void {
    if (!mobileOpen) return;
    mobileOpen = false;
    shell.dataset.mobileOpen = "false";
    const toggle = shell.querySelector<HTMLButtonElement>('[data-ui2-control="sidebar"]');
    toggle?.setAttribute("aria-expanded", "false");
    const scrim = shell.querySelector<HTMLElement>(".ui2-shell-scrim");
    if (scrim) scrim.hidden = true;
    if (restoreFocus) toggle?.focus();
  }

  function navigate(path: string): void {
    if (disposed) return;
    const next = resolveUi2Route(path);
    if (!next.recognized) return;
    route = next;
    const hash = ui2HashForPath(next.path);
    if (win.location.hash !== hash) win.location.hash = hash;
    mobileOpen = false;
    render("heading");
  }

  const onHashChange = () => {
    if (disposed) return;
    const next = readUi2Hash(win.location.hash);
    if (route.path === next.path && route.recognized === next.recognized) return;
    route = next;
    mobileOpen = false;
    render("heading");
  };

  const onKeyDown = (event: KeyboardEvent) => {
    if (disposed) return;
    if ((event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === "k") {
      event.preventDefault();
      openSearch();
      return;
    }
    if (event.key === "Escape" && mobileOpen) {
      event.preventDefault();
      closeMobileNavigation(true);
      return;
    }
    const nav = (event.target as Element | null)?.closest?.(".ui2-shell-navigation");
    if (!nav) return;
    const buttons = Array.from(nav.querySelectorAll<HTMLButtonElement>("button:not(:disabled)"));
    const index = buttons.indexOf(event.target as HTMLButtonElement);
    if (index < 0) return;
    let next = index;
    if (event.key === "ArrowDown") next = (index + 1) % buttons.length;
    else if (event.key === "ArrowUp") next = (index + buttons.length - 1) % buttons.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = buttons.length - 1;
    else if (event.key === "Tab" && mobileOpen && isNarrow()) {
      // The drawer is modal on narrow windows; cycle keyboard focus inside.
      if (event.shiftKey && index === 0) next = buttons.length - 1;
      else if (!event.shiftKey && index === buttons.length - 1) next = 0;
      else return;
    } else return;
    event.preventDefault();
    buttons[next]?.focus();
  };

  function openSearch(): void {
    if (disposed || searchDialog?.isOpen()) return;
    closeMobileNavigation(false);
    const trigger = shell.querySelector<HTMLButtonElement>('[data-ui2-control="search"]') ?? undefined;
    searchDialog?.open(trigger);
  }

  function render(focus: "none" | "heading" | "language" = "none"): void {
    if (disposed) return;
    searchDialog?.destroy();
    searchDialog = undefined;
    const activeControl = (doc.activeElement as HTMLElement | null)?.getAttribute("data-ui2-control");
    shell.replaceChildren();
    doc.documentElement.lang = i18n.getLocale() === "id" ? "id" : "en";
    shell.dataset.collapsed = String(collapsed);
    shell.dataset.mobileOpen = String(mobileOpen);
    shell.dataset.activeRoute = route.destination;

    const skip = doc.createElement("a");
    skip.href = "#ui2-main";
    skip.className = "ui2-shell-skip";
    skip.textContent = i18n.t("app.skipToMain");
    skip.addEventListener("click", (event) => {
      event.preventDefault();
      focusAfterRender("#ui2-main");
    });

    const aside = doc.createElement("aside");
    aside.className = "ui2-shell-sidebar";
    aside.setAttribute("aria-label", i18n.t("shell.navAria"));
    const brand = doc.createElement("div");
    brand.className = "ui2-shell-brand";
    brand.setAttribute("aria-label", i18n.t("app.name"));
    const logo = doc.createElement("span");
    logo.className = "ui2-shell-mark";
    logo.setAttribute("aria-hidden", "true");
    logo.textContent = "m";
    brand.append(logo);
    aside.append(brand);

    const nav = doc.createElement("nav");
    nav.className = "ui2-shell-navigation";
    nav.setAttribute("aria-label", i18n.t("shell.navAria"));
    for (const item of UI2_SHELL_DESTINATIONS) {
      const button = doc.createElement("button");
      button.type = "button";
      button.className = "ui2-shell-nav-button";
      button.dataset.ui2Route = item.path;
      button.setAttribute("aria-label", i18n.t(item.labelKey));
      button.title = i18n.t(item.labelKey);
      if (item.destination === route.destination) button.setAttribute("aria-current", "page");
      button.append(createUi2Icon(doc, item.icon, 18));
      const label = doc.createElement("span");
      label.className = "ui2-shell-nav-label";
      label.textContent = i18n.t(item.labelKey);
      button.append(label);
      button.addEventListener("click", () => navigate(item.path));
      nav.append(button);
    }
    aside.append(nav);

    const scrim = doc.createElement("button");
    scrim.type = "button";
    scrim.className = "ui2-shell-scrim";
    scrim.setAttribute("aria-label", i18n.t("shell.closeSidebar"));
    scrim.hidden = !mobileOpen;
    scrim.addEventListener("click", () => closeMobileNavigation(true));

    const mainColumn = doc.createElement("div");
    mainColumn.className = "ui2-shell-main-column";
    const bar = doc.createElement("header");
    bar.className = "ui2-shell-topbar";

    const toggle = createUi2Button(doc, {
      label: i18n.t("top.toggleSidebar"), ariaLabel: i18n.t("top.toggleSidebar"),
      variant: "ghost", icon: "panel",
      onClick: () => {
        if (isNarrow()) {
          mobileOpen = !mobileOpen;
          shell.dataset.mobileOpen = String(mobileOpen);
          scrim.hidden = !mobileOpen;
          toggle.setAttribute("aria-expanded", String(mobileOpen));
          if (mobileOpen) focusAfterRender(".ui2-shell-nav-button");
          else toggle.focus();
        } else {
          collapsed = !collapsed;
          shell.dataset.collapsed = String(collapsed);
          toggle.setAttribute("aria-expanded", String(!collapsed));
        }
      },
    });
    toggle.classList.add("ui2-shell-icon-button");
    toggle.dataset.ui2Control = "sidebar";
    toggle.setAttribute("aria-controls", "ui2-sidebar");
    toggle.setAttribute("aria-expanded", String(isNarrow() ? mobileOpen : !collapsed));
    aside.id = "ui2-sidebar";
    bar.append(toggle);

    const breadcrumb = createUi2Breadcrumb(doc, {
      ariaLabel: i18n.t("breadcrumb.ariaLabel"),
      items: route.breadcrumbs.map((step) => ({
        id: step.path,
        label: i18n.t(step.labelKey),
        onClick: () => navigate(step.path),
      })),
    });
    bar.append(breadcrumb);

    const search = createUi2Button(doc, {
      label: i18n.t("top.searchPlaceholder"), ariaLabel: i18n.t("top.searchPlaceholder"),
      variant: "secondary", icon: "search", onClick: openSearch,
    });
    search.classList.add("ui2-shell-search");
    search.dataset.ui2Control = "search";
    const shortcut = doc.createElement("kbd");
    shortcut.className = "ui2-shell-shortcut";
    shortcut.textContent = i18n.t("top.searchShortcut");
    search.append(shortcut);
    bar.append(search);

    const language = createUi2Segmented<Ui2TestLocale>(doc, {
      ariaLabel: i18n.t("top.languageLabel"),
      options: [
        { value: "en", label: i18n.t("lang.short.en") },
        { value: "id", label: i18n.t("lang.short.id") },
      ],
      value: i18n.getLocale(),
      onChange: (value) => i18n.setLocale(value),
    });
    language.classList.add("ui2-shell-language");
    language.dataset.ui2Control = "language";
    bar.append(language);

    const lock = createUi2Button(doc, {
      label: i18n.t("top.lock"), ariaLabel: i18n.t("top.lock"),
      variant: "ghost", icon: "lock", onClick: () => {
        lock.disabled = true;
        void options.onLock();
      },
    });
    lock.classList.add("ui2-shell-icon-button", "ui2-shell-lock");
    lock.dataset.ui2Control = "lock";
    bar.append(lock);
    mainColumn.append(bar);

    const main = doc.createElement("main");
    main.id = "ui2-main";
    main.className = "ui2-shell-view";
    main.tabIndex = -1;
    const header = doc.createElement("header");
    header.className = "ui2-header";
    const heading = doc.createElement("h1");
    heading.className = "ui2-h1";
    heading.tabIndex = -1;
    heading.dataset.ui2Control = "heading";
    const last = route.breadcrumbs[route.breadcrumbs.length - 1];
    heading.textContent = route.recognized ? i18n.t(last.labelKey) : i18n.t("shell.invalidTitle");
    header.append(heading);
    const lead = doc.createElement("p");
    lead.className = "ui2-lead";
    lead.textContent = i18n.t("shell.previewLead");
    header.append(lead);
    const placeholder = createUi2EmptyState(doc, {
      icon: "info", title: route.recognized ? i18n.t("shell.previewTitle") : i18n.t("shell.invalidTitle"),
      body: route.recognized ? i18n.t("shell.previewBody") : i18n.t("shell.invalidBody"),
      ...(options.onSwitchClassic
        ? { actionLabel: i18n.t("shell.openClassic"), onAction: options.onSwitchClassic }
        : {}),
    });
    placeholder.classList.add("ui2-shell-placeholder");
    main.append(header, placeholder);
    mainColumn.append(main);
    shell.append(skip, aside, scrim, mainColumn);

    searchDialog = createUi2Dialog(doc, {
      title: i18n.t("shell.searchTitle"),
      closeLabel: i18n.t("action.close"),
    });
    searchDialog.body.append(createUi2EmptyState(doc, {
      icon: "search", title: i18n.t("shell.searchUnavailableTitle"),
      body: i18n.t("shell.searchUnavailableBody"),
    }));
    shell.append(searchDialog.backdrop);
    if (focus === "heading") heading.focus();
    else if (focus === "language" || activeControl === "language") {
      focusAfterRender('[data-ui2-seg-value="' + i18n.getLocale() + '"]');
    }
  }

  const unsubscribe = i18n.subscribe(() => render("language"));
  doc.addEventListener("keydown", onKeyDown);
  win.addEventListener("hashchange", onHashChange);
  root.replaceChildren(shell);
  render();

  return {
    i18n,
    getRoute: () => route,
    navigate,
    destroy() {
      if (disposed) return;
      disposed = true;
      unsubscribe();
      doc.removeEventListener("keydown", onKeyDown);
      win.removeEventListener("hashchange", onHashChange);
      searchDialog?.destroy();
      shell.remove();
    },
  };
}

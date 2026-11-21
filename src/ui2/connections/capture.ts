import type { SetupInfo } from "../../contracts.js";
import type { Invoke } from "../../reader.js";
import type { Ui2I18nController, Ui2TranslationKey } from "../i18n/index.js";
import {
  createUi2Banner,
  createUi2Button,
  createUi2ListCard,
  createUi2ListRow,
  createUi2SettingsCard,
  createUi2Tag,
} from "../ui-kit/primitives.js";
import type { ConnectionsSection } from "./section.js";

const BROWSERS = ["chrome", "edge", "brave"] as const;
type BrowserId = (typeof BROWSERS)[number];

const BROWSER_LABEL: Record<BrowserId, Ui2TranslationKey> = {
  chrome: "conn.browser.chrome",
  edge: "conn.browser.edge",
  brave: "conn.browser.brave",
};

const PROVIDERS: ReadonlyArray<{ nameKey: Ui2TranslationKey; descKey: Ui2TranslationKey }> = [
  { nameKey: "grants.provider.chatgpt", descKey: "conn.p.chatgpt" },
  { nameKey: "grants.provider.claude", descKey: "conn.p.claude" },
  { nameKey: "grants.provider.gemini", descKey: "conn.p.gemini" },
];

export interface CaptureDeps {
  doc: Document;
  i18n: Ui2I18nController;
  invoke: Invoke;
}

/**
 * Connections > Save chats from your browser, and Saving options. No status is
 * shown that Mind Place cannot verify: a provider row says what it can do, and
 * a browser is "Registered" only when the native setup reports it. Automatic
 * save is set inside the browser extension (opt-in, loaded messages only,
 * never scrolls), so this page explains it and does not offer a switch.
 */
export function createCaptureSection(deps: CaptureDeps): ConnectionsSection {
  const { doc, i18n, invoke } = deps;
  const root = doc.createElement("div");
  root.className = "ui2-stack";

  let alive = true;
  let registered: ReadonlySet<string> | null = null;
  let setupState: "loading" | "ready" | "error" = "loading";
  const registering = new Set<BrowserId>();
  let message: string | null = null;

  function heading(key: Ui2TranslationKey): HTMLElement {
    const h = doc.createElement("h3");
    h.className = "ui2-h3";
    h.textContent = i18n.t(key);
    return h;
  }

  function browserRows(): HTMLElement {
    if (setupState === "error") {
      return createUi2Banner(doc, {
        tone: "danger",
        title: i18n.t("conn.setupErrorTitle"),
        message: i18n.t("conn.setupErrorBody"),
        actionLabel: i18n.t("action.retry"),
        onAction: () => void loadSetup(),
      });
    }
    return createUi2ListCard(
      doc,
      BROWSERS.map((id) => {
        const isRegistered = registered?.has(id) === true;
        const trailing =
          setupState === "loading" || registered === null
            ? undefined
            : isRegistered
              ? undefined
              : createUi2Button(doc, {
                  label: i18n.t("conn.register"),
                  variant: "secondary",
                  size: "sm",
                  disabled: registering.has(id),
                  onClick: () => void register(id),
                });
        return createUi2ListRow(doc, {
          title: i18n.t(BROWSER_LABEL[id]),
          ...(isRegistered ? { tag: { label: i18n.t("conn.registered"), tone: "ok" as const } } : {}),
          ...(trailing ? { trailing } : {}),
        });
      }),
    );
  }

  function render(): void {
    if (!alive) return;
    const parts: HTMLElement[] = [];
    if (message) {
      parts.push(createUi2Banner(doc, { tone: "danger", title: message, message: i18n.t("state.errorHint") }));
    }
    parts.push(
      heading("conn.saveH"),
      createUi2ListCard(
        doc,
        PROVIDERS.map((provider) =>
          createUi2ListRow(doc, { title: i18n.t(provider.nameKey), meta: i18n.t(provider.descKey) }),
        ),
      ),
      createUi2SettingsCard(doc, {
        title: i18n.t("conn.optH"),
        rows: [
          {
            title: i18n.t("conn.auto"),
            description: i18n.t("conn.autoV"),
            control: createUi2Tag(doc, { label: i18n.t("conn.autoWhere") }),
          },
          {
            title: i18n.t("conn.manual"),
            description: i18n.t("conn.manualV"),
            control: doc.createElement("span"),
          },
        ],
      }),
      heading("conn.browsers"),
      browserRows(),
    );
    root.replaceChildren(...parts);
  }

  async function loadSetup(): Promise<void> {
    setupState = "loading";
    render();
    try {
      const info = await invoke<SetupInfo>("get_setup");
      if (!alive) return;
      registered = new Set(info.registered_browsers);
      setupState = "ready";
    } catch {
      if (!alive) return;
      setupState = "error";
    }
    render();
  }

  async function register(browser: BrowserId): Promise<void> {
    registering.add(browser);
    message = null;
    render();
    try {
      const info = await invoke<SetupInfo>("register_extension", { browser });
      if (!alive) return;
      registered = new Set(info.registered_browsers);
    } catch {
      message = i18n.t("conn.registerError");
    }
    registering.delete(browser);
    render();
  }

  render();
  void loadSetup();

  return {
    element: root,
    rebuild: render,
    destroy() {
      alive = false;
      root.replaceChildren();
    },
  };
}

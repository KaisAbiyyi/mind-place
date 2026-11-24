import { listMcpClientSources } from "../../mcp-read-grants.js";
import type { ClipboardWriter, Invoke } from "../../reader.js";
import type { Ui2I18nController } from "../i18n/index.js";
import {
  createUi2Banner,
  createUi2Button,
  createUi2Card,
  createUi2Disclosure,
  createUi2Field,
  createUi2InlineConfirm,
  createUi2ListCard,
  createUi2ListRow,
  createUi2SettingsCard,
  createUi2Switch,
  renderUi2TechnicalCode,
  type Ui2DialogHandle,
} from "../ui-kit/primitives.js";
import { openChooseChatsDialog } from "./choose-chats-dialog.js";
import type { ConnectionsSection } from "./section.js";

interface McpStatus {
  running: boolean;
}
interface IssuedKey {
  client_id: string;
  credential: string;
}
interface ClientRow {
  id: string;
  display_name: string;
  state: string;
}

const PAGE_SIZE = 50;

export interface AiAccessDeps {
  doc: Document;
  i18n: Ui2I18nController;
  invoke: Invoke;
  clipboard?: ClipboardWriter;
  /** Collapsed "Advanced" disclosure shown at the bottom of this card. */
  hosted?: ConnectionsSection;
}

/**
 * Connections > Let an AI app read your memory. Consent, revoke and
 * credential handling are the same as the classic screen: the key is shown
 * once, never stored, and cleared when the page is left or the app locks
 * (the shell destroys the page). A running listener is "sharing is on", never
 * "connected": nothing here verifies a client actually reached Mind Place.
 */
export function createAiAccessSection(deps: AiAccessDeps): ConnectionsSection {
  const { doc, i18n, invoke } = deps;
  const root = doc.createElement("section");
  root.className = "ui2-stack";

  let alive = true;
  let running: boolean | null = null;
  let sharingBusy = false;
  let creating = false;
  let message: string | null = null;
  let nameValue = "";
  let issued: IssuedKey | null = null;
  let copied = false;
  let copyFailed = false;
  let clients: ClientRow[] = [];
  let hasMore = false;
  let listState: "loading" | "ready" | "error" = "loading";
  const counts = new Map<string, number | null>();
  let openDialog: Ui2DialogHandle | undefined;

  const live = () => alive && root.isConnected !== false;

  // Keyboard focus survives re-renders. The card is rebuilt on every state change,
  // which would otherwise drop focus to the page body after each key press.
  let pendingFocus: string | null = null;
  const mark = <T extends HTMLElement>(element: T, key: string): T => {
    element.dataset.connFocus = key;
    return element;
  };
  // Opaque per-app focus keys: client ids never appear in the page.
  const appKeys = new Map<string, string>();
  const appKey = (clientId: string): string => {
    let key = appKeys.get(clientId);
    if (!key) {
      key = `a${appKeys.size + 1}`;
      appKeys.set(clientId, key);
    }
    return key;
  };
  const keyed = (key: string) =>
    [...root.querySelectorAll<HTMLElement>("[data-conn-focus]")].find((element) => element.dataset.connFocus === key);
  const focusable = (holder: HTMLElement): HTMLElement | null => {
    const target = holder.matches("button, input, [tabindex]")
      ? holder
      : holder.querySelector<HTMLElement>("button:not([hidden]):not(:disabled), input:not(:disabled)");
    return target && !(target as HTMLButtonElement).disabled && !target.hidden ? target : null;
  };
  /** Where focus is now: a keyed control of this card, a node of the persistent hosted section, or nothing. */
  function captureFocus(): { key: string | null; hosted: HTMLElement | null; range: [number | null, number | null] } {
    const active = doc.activeElement as HTMLElement | null;
    if (!active || active === doc.body) return { key: pendingFocus, hosted: null, range: [null, null] };
    if (!root.contains(active)) {
      pendingFocus = null;
      return { key: null, hosted: null, range: [null, null] };
    }
    pendingFocus = null;
    if (deps.hosted?.element.contains(active)) {
      const input = active as HTMLInputElement;
      return { key: null, hosted: active, range: [input.selectionStart ?? null, input.selectionEnd ?? null] };
    }
    return { key: active.closest<HTMLElement>("[data-conn-focus]")?.dataset.connFocus ?? null, hosted: null, range: [null, null] };
  }
  function restoreFocus(saved: ReturnType<typeof captureFocus>): void {
    if (saved.hosted && saved.hosted.isConnected) {
      saved.hosted.focus();
      const input = saved.hosted as HTMLInputElement;
      if (saved.range[0] !== null && saved.range[1] !== null) {
        try { input.setSelectionRange(saved.range[0], saved.range[1]); } catch { /* not a text field */ }
      }
      return;
    }
    if (!saved.key) return;
    const holder = keyed(saved.key);
    const target = holder ? focusable(holder) : null;
    if (target) {
      target.focus();
      pendingFocus = null;
      return;
    }
    // The control is temporarily disabled: try again after the next render.
    if (holder) {
      pendingFocus = saved.key;
      return;
    }
    // The control is gone (for example a revoked app): land on the list heading.
    pendingFocus = null;
    keyed("apps")?.focus();
  }

  function render(): void {
    if (!alive) return;
    const saved = captureFocus();
    const card = createUi2Card(doc);
    const body = card.body;

    if (message) {
      body.append(createUi2Banner(doc, { tone: "danger", title: message, message: i18n.t("state.errorHint") }));
    }

    // Sharing switch (starts or stops the local listener).
    const sharing = createUi2Switch(doc, {
      ariaLabel: i18n.t("conn.shareSwitch"),
      checked: running === true,
      disabled: running === null || sharingBusy,
      onChange: (next) => void changeSharing(next),
    });
    mark(sharing, "share");
    const sharingCard = createUi2SettingsCard(doc, {
      title: i18n.t("conn.aiH"),
      rows: [
        {
          title: running === null ? i18n.t("state.unknown") : i18n.t(running ? "conn.shareOn" : "conn.shareOff"),
          description: i18n.t("conn.shareV"),
          control: sharing,
        },
      ],
    });

    // Create access.
    const form = doc.createElement("div");
    form.className = "ui2-row-flex";
    const field = createUi2Field(doc, {
      label: i18n.t("conn.appName"),
      value: nameValue,
      onInput: (value) => {
        nameValue = value;
      },
    });
    field.element.style.flex = "1";
    const create = createUi2Button(doc, {
      label: i18n.t("conn.create"),
      variant: "primary",
      disabled: creating,
      onClick: () => void createAccess(),
    });
    mark(create, "create");
    form.append(field.element, create);
    body.append(form);

    if (issued) body.append(renderKey(issued));

    // Apps with access.
    const label = doc.createElement("p");
    label.className = "ui2-field-label";
    label.textContent = i18n.t("conn.apps");
    label.tabIndex = -1;
    mark(label, "apps");
    body.append(label);
    body.append(renderApps());

    const revoked = clients.filter((client) => client.state === "revoked");
    if (revoked.length > 0) {
      const list = doc.createElement("div");
      list.className = "ui2-stack-sm";
      const note = doc.createElement("p");
      note.className = "ui2-field-hint";
      note.textContent = i18n.t("conn.revokedV");
      list.append(
        note,
        createUi2ListCard(
          doc,
          revoked.map((client) => createUi2ListRow(doc, { title: client.display_name })),
        ),
      );
      body.append(createUi2Disclosure(doc, { summaryLabel: i18n.t("conn.revoked", { n: revoked.length }), body: list }));
    }

    if (deps.hosted) body.append(deps.hosted.element);
    root.replaceChildren(sharingCard, card.element);
    restoreFocus(saved);
  }

  function renderKey(key: IssuedKey): HTMLElement {
    const box = doc.createElement("div");
    box.className = "ui2-stack-sm";
    box.append(createUi2Banner(doc, { tone: "ok", title: i18n.t("conn.keyT"), message: i18n.t("conn.keyV") }));
    const code = renderUi2TechnicalCode(doc, key.credential);
    code.setAttribute("aria-label", i18n.t("conn.keyLabel"));
    const clipboard = deps.clipboard;
    const copy = createUi2Button(doc, {
      label: i18n.t(copied ? "action.copied" : "action.copy"),
      variant: "secondary",
      size: "sm",
      disabled: !clipboard,
      onClick: () => {
        if (!clipboard || !live() || issued !== key || !copy.isConnected) return;
        void (async () => {
          let success = false;
          try {
            await clipboard.writeText(key.credential);
            success = true;
          } catch {
            // A rejected write leaves the one-time key available for manual copy.
          }
          if (!live() || issued !== key) return;
          copied = success;
          copyFailed = !success;
          render();
        })();
      },
    });
    mark(copy, "copy");
    box.append(code);
    if (clipboard) box.append(copy);
    if (!clipboard || copyFailed) {
      const hint = doc.createElement("p");
      hint.className = "ui2-field-hint";
      hint.setAttribute("role", "status");
      hint.textContent = i18n.t("conn.copyManual");
      const select = createUi2Button(doc, {
        label: i18n.t("conn.selectKey"),
        variant: "secondary",
        size: "sm",
        onClick: () => {
          if (!live() || issued !== key || !code.isConnected) return;
          const selection = doc.getSelection();
          if (!selection) return;
          const range = doc.createRange();
          range.selectNodeContents(code);
          selection.removeAllRanges();
          selection.addRange(range);
        },
      });
      box.append(mark(select, "select-key"), hint);
    }
    return box;
  }

  function appsToShow(): ClientRow[] {
    return clients.filter((client) => client.state !== "revoked");
  }

  function renderApps(): HTMLElement {
    if (listState === "error" && clients.length === 0) {
      return createUi2Banner(doc, {
        tone: "danger",
        title: i18n.t("conn.listErrorTitle"),
        message: i18n.t("conn.listErrorBody"),
        actionLabel: i18n.t("action.retry"),
        onAction: () => void loadClients(true),
      });
    }
    const apps = appsToShow();
    if (apps.length === 0) {
      const empty = doc.createElement("p");
      empty.className = "ui2-field-hint";
      empty.textContent = listState === "loading" ? i18n.t("state.loading") : i18n.t("conn.noApps");
      return empty;
    }
    const wrap = doc.createElement("div");
    wrap.className = "ui2-stack-sm";
    wrap.append(
      createUi2ListCard(
        doc,
        apps.map((client) => {
          const actions = doc.createElement("div");
          actions.className = "ui2-row-flex";
          actions.append(
            mark(
              createUi2Button(doc, {
                label: i18n.t("conn.chooseChats"),
                variant: "secondary",
                size: "sm",
                onClick: (event) => openChoose(client, event.currentTarget as HTMLElement),
              }),
              `choose:${appKey(client.id)}`,
            ),
            mark(
              createUi2InlineConfirm(doc, {
                triggerLabel: i18n.t("action.revoke"),
                question: i18n.t("conn.revokeQuestion", { app: client.display_name }),
                confirmLabel: i18n.t("action.revoke"),
                cancelLabel: i18n.t("action.cancel"),
                onConfirm: () => void revoke(client),
              }).element,
              `revoke:${appKey(client.id)}`,
            ),
          );
          const count = counts.get(client.id);
          const tag =
            count === undefined || count === null
              ? undefined
              : count === 0
                ? { label: i18n.t("conn.allowedNone"), tone: "warn" as const }
                : { label: i18n.t("conn.allowed", { chats: i18n.tPlural("count.chats", count) }) };
          const row = createUi2ListRow(doc, {
            title: client.display_name,
            meta: i18n.t("conn.readOnly"),
            ...(tag ? { tag } : {}),
            trailing: actions,
          });
          row.classList.add("ui2-conn-app");
          return row;
        }),
      ),
    );
    if (hasMore) {
      wrap.append(
        createUi2Button(doc, {
          label: i18n.t("conn.more"),
          variant: "secondary",
          size: "sm",
          onClick: () => void loadClients(false),
        }),
      );
    }
    return wrap;
  }

  function openChoose(client: ClientRow, opener: HTMLElement): void {
    openDialog?.close();
    openDialog = openChooseChatsDialog({
      doc,
      i18n,
      invoke,
      client: { id: client.id, displayName: client.display_name },
      opener,
      onSaved: () => void refreshCount(client.id),
    });
  }

  async function refreshCount(clientId: string): Promise<void> {
    try {
      const sources = await listMcpClientSources(invoke, clientId);
      counts.set(clientId, sources.length);
    } catch {
      counts.set(clientId, null);
    }
    if (live()) render();
  }

  async function loadClients(reset: boolean): Promise<void> {
    if (reset) {
      clients = [];
      hasMore = false;
    }
    listState = "loading";
    render();
    try {
      const after = reset || clients.length === 0 ? undefined : clients[clients.length - 1].id;
      const page = await invoke<ClientRow[]>("list_local_mcp_clients", after ? { after } : {});
      if (!live()) return;
      clients = reset ? page : [...clients, ...page];
      hasMore = page.length >= PAGE_SIZE;
      listState = "ready";
      render();
      for (const client of page) {
        if (client.state !== "revoked" && !counts.has(client.id)) void refreshCount(client.id);
      }
    } catch {
      if (!live()) return;
      listState = "error";
      render();
    }
  }

  async function changeSharing(next: boolean): Promise<void> {
    sharingBusy = true;
    message = null;
    try {
      const result = await invoke<McpStatus>(next ? "start_local_mcp" : "stop_local_mcp");
      running = result.running;
    } catch {
      message = i18n.t("conn.shareError");
    }
    sharingBusy = false;
    if (live()) render();
  }

  async function createAccess(): Promise<void> {
    creating = true;
    message = null;
    render();
    const name = nameValue.trim() || i18n.t("conn.defaultName");
    try {
      const result = await invoke<IssuedKey>("issue_local_mcp_reader", { displayName: name });
      if (!alive) return;
      issued = { client_id: result.client_id, credential: result.credential };
      copied = false;
      copyFailed = false;
      nameValue = "";
      creating = false;
      await loadClients(true);
    } catch {
      creating = false;
      message = i18n.t("conn.createError");
      if (live()) render();
    }
  }

  async function revoke(client: ClientRow): Promise<void> {
    try {
      await invoke("revoke_local_mcp_client", { clientId: client.id });
      if (issued?.client_id === client.id) issued = null;
      counts.delete(client.id);
      message = null;
      await loadClients(true);
      try {
        const status = await invoke<McpStatus>("get_local_mcp_status");
        running = status.running;
      } catch {
        // Keep the last known state; revoking may also stop a hosted tunnel.
      }
    } catch {
      message = i18n.t("conn.revokeError");
    }
    if (live()) render();
  }

  render();
  void invoke<McpStatus>("get_local_mcp_status")
    .then((status) => {
      running = status.running;
      if (live()) render();
    })
    .catch(() => {
      if (live()) render();
    });
  void loadClients(true);

  return {
    element: root,
    rebuild() {
      render();
      deps.hosted?.rebuild();
    },
    destroy() {
      alive = false;
      deps.hosted?.destroy();
      openDialog?.close();
      openDialog = undefined;
      // Clear the one-time key and the typed name; nothing here is persisted.
      issued = null;
      copied = false;
      copyFailed = false;
      nameValue = "";
      appKeys.clear();
      counts.clear();
      clients = [];
      root.replaceChildren();
    },
  };
}

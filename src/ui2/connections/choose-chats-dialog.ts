import type { Invoke } from "../../reader.js";
import {
  grantMcpClientSources,
  listGrantableChats,
  revokeAllMcpClientSources,
  revokeMcpClientSources,
  type GrantableChat,
} from "../../mcp-read-grants.js";
import type { Ui2I18nController } from "../i18n/index.js";
import {
  createUi2Banner,
  createUi2Button,
  createUi2Dialog,
  createUi2EmptyState,
  createUi2Field,
  createUi2InlineConfirm,
  createUi2ListCard,
  createUi2ListRow,
  createUi2LoadingSkeleton,
  createUi2Switch,
  type Ui2DialogHandle,
} from "../ui-kit/primitives.js";

/**
 * "Choose chats" dialog for one AI app (MP-7MCP-02, Connections > AI app
 * access). Add-dialog pattern with two steps: Choose, then Review. Nothing is
 * changed before the owner approves the Review step. Source ids are opaque
 * handles that are sent to the native commands and never displayed.
 */
export interface ChooseChatsOptions {
  doc: Document;
  i18n: Ui2I18nController;
  invoke: Invoke;
  client: { id: string; displayName: string };
  opener?: HTMLElement | null;
  /** Where to mount; defaults to the UI2 root so tokens, font and reset apply. */
  parent?: HTMLElement;
  /** Called once after access was saved or removed, so the host can refresh. */
  onSaved?: () => void;
}

const PAGE_SIZE = 25;
const SEARCH_DELAY_MS = 250;

export function openChooseChatsDialog(options: ChooseChatsOptions): Ui2DialogHandle {
  const { doc, i18n, invoke, client } = options;
  const app = client.displayName;

  const dialog = createUi2Dialog(doc, {
    title: i18n.t("grants.title", { app }),
    closeLabel: i18n.t("action.close"),
    onClose: () => dialog.destroy(),
  });

  // Server truth for every chat seen so far, and the owner's pending changes
  // (desired state, only where it differs from the server).
  const known = new Map<string, GrantableChat>();
  const pending = new Map<string, boolean>();
  let query = "";
  let total = 0;
  let allowed = 0;
  let loaded = 0;
  let loadState: "loading" | "ready" | "error" = "loading";
  let requestToken = 0;
  let searchTimer: ReturnType<typeof setTimeout> | undefined;

  const providerLabel = (provider: string): string => {
    switch (provider) {
      case "chatgpt":
        return i18n.t("grants.provider.chatgpt");
      case "claude":
        return i18n.t("grants.provider.claude");
      case "gemini":
        return i18n.t("grants.provider.gemini");
      default:
        return i18n.t("grants.provider.generic");
    }
  };
  const metaFor = (chat: GrantableChat): string =>
    i18n.t("grants.rowMeta", {
      provider: providerLabel(chat.provider),
      date: i18n.formatDate(chat.captured_at, "medium"),
    });

  const desired = (chat: GrantableChat): boolean => pending.get(chat.source_id) ?? chat.granted;

  const body = dialog.body;
  const footer = dialog.footer;

  // ----- Choose step ------------------------------------------------------
  const lead = doc.createElement("p");
  lead.className = "ui2-lead";
  lead.textContent = i18n.t("grants.lead", { app });

  const search = createUi2Field(doc, {
    label: i18n.t("grants.search"),
    hint: i18n.t("grants.searchHint"),
    type: "search",
    onInput: (value) => {
      if (searchTimer) clearTimeout(searchTimer);
      searchTimer = setTimeout(() => {
        query = value.trim();
        void load(true);
      }, SEARCH_DELAY_MS);
    },
  });

  const summary = doc.createElement("p");
  summary.className = "ui2-field-hint";
  summary.setAttribute("role", "status");

  const listHost = doc.createElement("div");
  listHost.className = "ui2-stack-sm";

  const moreButton = createUi2Button(doc, {
    label: i18n.t("grants.more"),
    variant: "secondary",
    size: "sm",
    onClick: () => void load(false),
  });
  moreButton.hidden = true;

  const removeAll = createUi2InlineConfirm(doc, {
    triggerLabel: i18n.t("grants.removeAllTrigger"),
    question: i18n.t("grants.removeAllQuestion", { app }),
    confirmLabel: i18n.t("grants.removeAllConfirm"),
    cancelLabel: i18n.t("action.cancel"),
    onConfirm: () => {
      void (async () => {
        try {
          await revokeAllMcpClientSources(invoke, client.id);
          pending.clear();
          options.onSaved?.();
          await load(true);
        } catch {
          showChooseError(i18n.t("grants.saveError"));
        }
      })();
    },
  });
  removeAll.element.hidden = true;

  const chooseMessage = doc.createElement("div");
  chooseMessage.hidden = true;

  const cancelButton = createUi2Button(doc, {
    label: i18n.t("action.cancel"),
    variant: "secondary",
    onClick: () => dialog.close(),
  });
  const reviewButton = createUi2Button(doc, {
    label: i18n.t("grants.review"),
    variant: "primary",
    disabled: true,
    onClick: () => renderReview(),
  });

  function showChooseError(message: string): void {
    chooseMessage.hidden = false;
    chooseMessage.replaceChildren(
      createUi2Banner(doc, { tone: "danger", title: message, message: i18n.t("state.errorHint") }),
    );
  }

  function updateChrome(): void {
    summary.textContent =
      loadState === "ready" ? i18n.t("grants.summary", { count: allowed, total }) : "";
    reviewButton.disabled = pending.size === 0;
    removeAll.element.hidden = !(loadState === "ready" && allowed > 0);
    moreButton.hidden = !(loadState === "ready" && loaded < total);
  }

  function renderRows(rows: readonly GrantableChat[]): HTMLElement {
    return createUi2ListCard(
      doc,
      rows.map((chat) => {
        const sw = createUi2Switch(doc, {
          ariaLabel: i18n.t("grants.rowSwitch", { title: chat.title }),
          checked: desired(chat),
          onChange: (checked) => {
            if (checked === chat.granted) pending.delete(chat.source_id);
            else pending.set(chat.source_id, checked);
            updateChrome();
          },
        });
        return createUi2ListRow(doc, { title: chat.title, meta: metaFor(chat), trailing: sw });
      }),
    );
  }

  const shownRows: GrantableChat[] = [];

  function paintList(): void {
    if (loadState === "loading" && shownRows.length === 0) {
      listHost.replaceChildren(createUi2LoadingSkeleton(doc, i18n.t("state.loading"), i18n));
      return;
    }
    if (loadState === "error" && shownRows.length === 0) {
      listHost.replaceChildren(
        createUi2Banner(doc, {
          tone: "danger",
          title: i18n.t("grants.loadErrorTitle"),
          message: i18n.t("grants.loadErrorBody"),
          actionLabel: i18n.t("action.retry"),
          onAction: () => void load(true),
        }),
      );
      return;
    }
    if (shownRows.length === 0) {
      listHost.replaceChildren(
        query
          ? createUi2EmptyState(doc, {
              icon: "search",
              title: i18n.t("grants.noMatchTitle"),
              body: i18n.t("grants.noMatchBody"),
            })
          : createUi2EmptyState(doc, {
              icon: "chat",
              title: i18n.t("grants.emptyTitle"),
              body: i18n.t("grants.emptyBody"),
            }),
      );
      return;
    }
    listHost.replaceChildren(renderRows(shownRows));
  }

  async function load(reset: boolean): Promise<void> {
    const token = ++requestToken;
    if (reset) {
      shownRows.length = 0;
      loaded = 0;
    }
    loadState = "loading";
    chooseMessage.hidden = true;
    chooseMessage.replaceChildren();
    paintList();
    updateChrome();
    try {
      const page = await listGrantableChats(invoke, client.id, {
        query,
        offset: loaded,
        limit: PAGE_SIZE,
      });
      if (token !== requestToken) return;
      total = page.total;
      allowed = page.allowed;
      for (const chat of page.rows) {
        known.set(chat.source_id, chat);
        shownRows.push(chat);
      }
      loaded += page.rows.length;
      loadState = "ready";
    } catch {
      if (token !== requestToken) return;
      loadState = "error";
    }
    paintList();
    updateChrome();
  }

  function renderChoose(): void {
    body.replaceChildren(lead, search.element, summary, chooseMessage, listHost, moreButton, removeAll.element);
    footer.replaceChildren(cancelButton, reviewButton);
    updateChrome();
  }

  // ----- Review step ------------------------------------------------------
  function changed(wanted: boolean): GrantableChat[] {
    const rows: GrantableChat[] = [];
    for (const [id, value] of pending) {
      const chat = known.get(id);
      if (chat && value === wanted) rows.push(chat);
    }
    return rows;
  }

  function renderReview(): void {
    const adds = changed(true);
    const removes = changed(false);
    const head = doc.createElement("h3");
    head.className = "ui2-h3";
    head.textContent = i18n.t("grants.reviewTitle");
    const intro = doc.createElement("p");
    intro.className = "ui2-lead";
    intro.textContent = i18n.t("grants.reviewBody", { app });
    const reviewMessage = doc.createElement("div");
    reviewMessage.hidden = true;
    const sections: HTMLElement[] = [head, intro];
    const section = (key: "grants.willAllow" | "grants.willRemove", rows: GrantableChat[]) => {
      if (rows.length === 0) return;
      const line = doc.createElement("p");
      line.className = "ui2-field-label";
      line.textContent = i18n.t(key, { chats: i18n.tPlural("count.chats", rows.length) });
      sections.push(
        line,
        createUi2ListCard(
          doc,
          rows.map((chat) => createUi2ListRow(doc, { title: chat.title, meta: metaFor(chat) })),
        ),
      );
    };
    section("grants.willAllow", adds);
    section("grants.willRemove", removes);
    sections.push(reviewMessage);
    body.replaceChildren(...sections);

    const back = createUi2Button(doc, {
      label: i18n.t("action.back"),
      variant: "secondary",
      onClick: () => renderChoose(),
    });
    const approve = createUi2Button(doc, {
      label: i18n.t("grants.approve"),
      variant: "primary",
      onClick: () => {
        approve.disabled = true;
        back.disabled = true;
        void (async () => {
          try {
            if (adds.length > 0) {
              await grantMcpClientSources(invoke, client.id, adds.map((c) => c.source_id));
            }
            if (removes.length > 0) {
              await revokeMcpClientSources(invoke, client.id, removes.map((c) => c.source_id));
            }
            pending.clear();
            options.onSaved?.();
            dialog.close();
          } catch {
            reviewMessage.hidden = false;
            reviewMessage.replaceChildren(
              createUi2Banner(doc, {
                tone: "danger",
                title: i18n.t("grants.saveError"),
                message: i18n.t("state.errorHint"),
              }),
            );
            approve.disabled = false;
            back.disabled = false;
          }
        })();
      },
    });
    footer.replaceChildren(back, approve);
    approve.focus();
  }

  renderChoose();
  (options.parent ?? doc.querySelector<HTMLElement>(".ui2-root") ?? doc.body).append(dialog.backdrop);
  dialog.open(options.opener ?? null);
  // Start in the search field, not on the close button.
  search.input.focus();
  void load(true);
  return dialog;
}

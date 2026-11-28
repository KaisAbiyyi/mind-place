import type { ConversationSummary } from "../../contracts.js";
import type { Ui2I18nController } from "../i18n/index.js";
import {
  createUi2Button, createUi2ChipGroup, createUi2EmptyState, createUi2ListCard, createUi2ListRow,
  createUi2LoadingSkeleton, type Ui2ChipGroupItem,
} from "../ui-kit/index.js";
import { providerKey, providerName } from "./format.js";

/** Rows drawn at once. A bigger archive is paged, never dumped. */
export const ARCHIVE_PAGE_SIZE = 50;

/** The providers the guide always shows as chips. Others found in the archive follow. */
const DEFAULT_PROVIDERS = ["chatgpt", "claude", "gemini"] as const;

export interface ArchiveListDeps {
  doc: Document;
  i18n: Ui2I18nController;
  onOpen(summary: ConversationSummary): void;
  onRetry(): void;
  /** Present only when the shell can open another destination. */
  onSetup?: (() => void) | undefined;
}

export interface ArchiveListHandle {
  readonly element: HTMLElement;
  setLoading(): void;
  setError(): void;
  setSummaries(items: readonly ConversationSummary[]): void;
  /** Repaint (language change) keeping the filter and page. */
  rebuild(): void;
  /** Focus the row for a chat, if it is on the current page. */
  focusRow(id: string): boolean;
  destroy(): void;
}

type Phase = "loading" | "error" | "ready";

export function createArchiveList(deps: ArchiveListDeps): ArchiveListHandle {
  const { doc, i18n } = deps;
  const element = doc.createElement("div");
  element.className = "ui2-stack";

  let phase: Phase = "loading";
  let summaries: readonly ConversationSummary[] = [];
  let filter = "all";
  let pageIndex = 0;
  // Row elements by chat id. Ids are never written to the DOM.
  let rows = new Map<string, HTMLElement>();

  const resultsHost = doc.createElement("div");
  resultsHost.className = "ui2-stack";
  let previous: HTMLButtonElement | undefined;
  let next: HTMLButtonElement | undefined;
  let range: HTMLElement | undefined;
  let pager: HTMLElement | undefined;

  function providersInArchive(): string[] {
    const keys: string[] = [...DEFAULT_PROVIDERS];
    for (const item of summaries) {
      const key = providerKey(item.provider);
      if (key && !keys.includes(key)) keys.push(key);
    }
    return keys;
  }

  function visible(): ConversationSummary[] {
    return filter === "all" ? [...summaries] : summaries.filter((item) => providerKey(item.provider) === filter);
  }

  function updatePager(total: number, from: number, to: number): void {
    if (!pager || !previous || !next || !range) return;
    range.textContent = i18n.t("arch.range", {
      from: i18n.formatNumber(from), to: i18n.formatNumber(to), total: i18n.formatNumber(total),
    });
    pager.hidden = total <= ARCHIVE_PAGE_SIZE;
    previous.disabled = pageIndex === 0;
    next.disabled = to >= total;
  }

  function renderResults(): void {
    rows = new Map();
    resultsHost.replaceChildren();
    const matching = visible();
    if (matching.length === 0) {
      resultsHost.append(createUi2EmptyState(doc, {
        icon: "chat",
        title: i18n.t("arch.emptyAppTitle", { app: providerName(filter) }),
        body: i18n.t("arch.emptyBody"),
      }));
      return;
    }
    const pages = Math.ceil(matching.length / ARCHIVE_PAGE_SIZE);
    pageIndex = Math.max(0, Math.min(pageIndex, pages - 1));
    const start = pageIndex * ARCHIVE_PAGE_SIZE;
    const slice = matching.slice(start, start + ARCHIVE_PAGE_SIZE);
    const listRows = slice.map((item) => {
      const row = createUi2ListRow(doc, {
        title: item.title || i18n.t("arch.untitled"),
        meta: i18n.t("arch.rowMeta", {
          app: providerName(item.provider) || i18n.t("state.unknown"),
          date: i18n.formatDate(item.captured_at, "medium"),
        }),
        ...(item.completeness === "partial" ? { tag: { label: i18n.t("arch.partial"), tone: "warn" as const } } : {}),
        onClick: () => deps.onOpen(item),
      });
      rows.set(item.id, row);
      return row;
    });
    resultsHost.append(createUi2ListCard(doc, listRows));

    pager = doc.createElement("nav");
    pager.className = "ui2-chats-pager";
    pager.setAttribute("aria-label", i18n.t("arch.pagerLabel"));
    range = doc.createElement("span");
    range.className = "ui2-chats-range";
    range.setAttribute("role", "status");
    const go = (delta: number) => {
      pageIndex += delta;
      renderResults();
      // The pager is rebuilt: keep keyboard focus on the button that was used, or its
      // partner when that one just reached the end.
      const used = delta > 0 ? next : previous;
      const partner = delta > 0 ? previous : next;
      (used && !used.disabled ? used : partner)?.focus();
    };
    previous = createUi2Button(doc, { label: i18n.t("arch.prev"), size: "sm" });
    next = createUi2Button(doc, { label: i18n.t("arch.next"), size: "sm" });
    previous.addEventListener("click", () => go(-1));
    next.addEventListener("click", () => go(1));
    pager.append(previous, range, next);
    resultsHost.append(pager);
    updatePager(matching.length, start + 1, Math.min(start + ARCHIVE_PAGE_SIZE, matching.length));
  }

  function render(): void {
    rows = new Map();
    element.replaceChildren();
    if (phase === "loading") {
      element.append(createUi2LoadingSkeleton(doc, i18n.t("state.loading"), i18n));
      return;
    }
    if (phase === "error") {
      element.append(createUi2EmptyState(doc, {
        icon: "info",
        title: i18n.t("arch.listErrorTitle"),
        body: i18n.t("arch.listErrorBody"),
        actionLabel: i18n.t("action.retry"),
        onAction: deps.onRetry,
      }));
      return;
    }
    if (summaries.length === 0) {
      element.append(createUi2EmptyState(doc, {
        icon: "chat",
        title: i18n.t("arch.emptyTitle"),
        body: i18n.t("arch.emptyBody"),
        ...(deps.onSetup ? { actionLabel: i18n.t("arch.emptyAction"), onAction: deps.onSetup } : {}),
      }));
      return;
    }
    const items: Ui2ChipGroupItem[] = [
      { id: "all", label: i18n.t("arch.all") },
      ...providersInArchive().map((key) => ({ id: key, label: providerName(key) })),
    ];
    if (!items.some((item) => item.id === filter)) filter = "all";
    element.append(
      createUi2ChipGroup(doc, {
        ariaLabel: i18n.t("arch.filterLabel"),
        items,
        activeId: filter,
        onChange: (id) => {
          filter = id;
          pageIndex = 0;
          renderResults();
        },
      }),
      resultsHost,
    );
    renderResults();
  }

  render();

  return {
    element,
    setLoading() { phase = "loading"; render(); },
    setError() { phase = "error"; render(); },
    setSummaries(items) {
      summaries = items;
      phase = "ready";
      pageIndex = 0;
      render();
    },
    rebuild: render,
    focusRow(id) {
      const row = rows.get(id);
      if (!row || !row.isConnected) return false;
      row.focus();
      return true;
    },
    destroy() {
      rows = new Map();
      summaries = [];
      element.replaceChildren();
      element.remove();
    },
  };
}

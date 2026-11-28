import type { Capture, CaptureSnapshot, Conversation, ConversationSummary } from "../../contracts.js";
import type { ClipboardWriter, Invoke } from "../../reader.js";
import type { Ui2I18nController } from "../i18n/index.js";
import {
  createUi2Aside, createUi2Button, createUi2EmptyState, createUi2LoadingSkeleton, type Ui2AsideHandle,
} from "../ui-kit/index.js";
import { createExportPanel, type ExportPanel } from "./export-panel.js";
import { COPIES_PAGE, buildInfoBody, type InfoBody, type InfoState } from "./info.js";
import { renderThreadMessage, transcriptPlainText, type ThreadDeps } from "./thread.js";

/** Messages drawn at once. Longer chats are paged so the window stays fast. */
export const MESSAGES_PER_PAGE = 40;

const COPIED_FLASH_MS = 1400;

export interface ChatReaderDeps {
  doc: Document;
  i18n: Ui2I18nController;
  invoke: Invoke;
  clipboard?: ClipboardWriter | undefined;
  /** The row the owner opened. Its title is shown until the saved copy arrives. */
  summary: ConversationSummary;
  onBack(): void;
  /** The saved copy has its own title: tell the shell for the breadcrumb. */
  onTitle(title: string): void;
}

export interface ChatReaderHandle {
  readonly element: HTMLElement;
  /** Moves keyboard focus to the chat title (used right after opening). */
  focusTitle(): void;
  /** A language change repaints from state without reading the archive again. */
  rebuild(): void;
  destroy(): void;
}

type Phase = "loading" | "ready" | "error";

/** The chat reader: a thread that looks like a chat app, with metadata behind Info. */
export function createChatReader(deps: ChatReaderDeps): ChatReaderHandle {
  const { doc, i18n, invoke, summary } = deps;
  const root = doc.createElement("div");
  root.className = "ui2-chats-reader";

  let alive = true;
  let phase: Phase = "loading";
  let loadVersion = 0;
  let renderGeneration = 0;
  let conversation: Conversation | null = null;
  let snapshots: CaptureSnapshot[] = [];
  let activeSnapshotId: string | null = null;
  let pageIndex = 0;
  let copiedTimer: ReturnType<typeof setTimeout> | undefined;
  const info: InfoState = { copiesShown: COPIES_PAGE, busy: false, copyFailed: false };

  let titleEl: HTMLElement | undefined;
  let infoAside: Ui2AsideHandle | undefined;
  let infoBody: InfoBody | undefined;
  let exportPanel: ExportPanel | undefined;
  let statusLine: HTMLElement | undefined;
  let copyButton: HTMLButtonElement | undefined;

  const title = (): string =>
    conversation?.capture.title || conversation?.summary.title || summary.title || i18n.t("arch.untitled");

  function clearCopiedTimer(): void {
    if (copiedTimer !== undefined) clearTimeout(copiedTimer);
    copiedTimer = undefined;
  }

  function setStatus(text: string): void {
    if (statusLine) statusLine.textContent = text;
  }

  function disposeAsides(): void {
    exportPanel?.destroy();
    exportPanel = undefined;
    infoAside?.element.remove();
    infoAside = undefined;
    infoBody = undefined;
  }

  function renderInfo(): void {
    if (!infoAside || !conversation) return;
    infoBody = buildInfoBody({
      doc, i18n,
      summary: conversation.summary,
      capture: conversation.capture,
      snapshots,
      activeSnapshotId,
      state: info,
      onSelectCopy: (id) => { void selectCopy(id); },
      onShowMoreCopies: () => {
        info.copiesShown += COPIES_PAGE;
        renderInfo();
      },
    });
    infoAside.body.replaceChildren(infoBody.element);
  }

  async function copyTranscript(): Promise<void> {
    if (!conversation || !copyButton) return;
    const text = transcriptPlainText(i18n, conversation.capture.messages, conversation.summary.provider);
    const writer = deps.clipboard ?? doc.defaultView?.navigator.clipboard;
    const label = copyButton.querySelector("span");
    try {
      if (!writer) throw new Error("MP_UI2_CLIPBOARD_UNAVAILABLE");
      await writer.writeText(text);
      if (!alive || !label) return;
      setStatus("");
      label.textContent = i18n.t("action.copied");
      clearCopiedTimer();
      copiedTimer = setTimeout(() => {
        if (label.isConnected) label.textContent = i18n.t("action.copy");
      }, COPIED_FLASH_MS);
    } catch {
      if (alive) setStatus(i18n.t("arch.copyError"));
    }
  }

  async function selectCopy(snapshotId: string): Promise<void> {
    if (info.busy || snapshotId === activeSnapshotId || !conversation) return;
    const version = ++loadVersion;
    info.busy = true;
    info.copyFailed = false;
    renderInfo();
    try {
      const result = await invoke<Conversation>("read_conversation", { id: summary.id, snapshotId });
      if (!alive || version !== loadVersion) return;
      if (result.summary.id !== summary.id) throw new Error("MP_UI2_CHAT_MISMATCH");
      conversation = result;
      activeSnapshotId = snapshotId;
      pageIndex = 0;
      exportPanel?.reset();
      info.busy = false;
      renderThread();
      renderInfo();
      infoAside?.body.querySelector<HTMLElement>("[data-ui2-copy-active]")?.focus();
    } catch {
      if (!alive || version !== loadVersion) return;
      info.busy = false;
      info.copyFailed = true;
      renderInfo();
    }
  }

  const threadHost = doc.createElement("div");
  threadHost.className = "ui2-chats-thread-host";

  function threadDeps(): ThreadDeps {
    const generation = renderGeneration;
    return {
      doc, i18n, invoke,
      archiveId: summary.id,
      provider: conversation?.summary.provider ?? summary.provider,
      isLive: () => alive && generation === renderGeneration,
    };
  }

  /** The visible window of messages plus the pager. Never more than one page in the DOM. */
  function renderThread(): void {
    renderGeneration += 1;
    threadHost.replaceChildren();
    if (!conversation) return;
    const messages = conversation.capture.messages;
    const thread = doc.createElement("div");
    thread.className = "ui2-chats-thread";
    if (messages.length === 0) {
      const empty = doc.createElement("p");
      empty.className = "ui2-chats-note";
      empty.textContent = i18n.t("arch.noMessages");
      thread.append(empty);
    }
    const pages = Math.max(1, Math.ceil(messages.length / MESSAGES_PER_PAGE));
    pageIndex = Math.max(0, Math.min(pageIndex, pages - 1));
    const start = pageIndex * MESSAGES_PER_PAGE;
    const deps2 = threadDeps();
    for (const message of messages.slice(start, start + MESSAGES_PER_PAGE)) {
      thread.append(renderThreadMessage(deps2, message));
    }
    threadHost.append(thread);

    if (messages.length > MESSAGES_PER_PAGE) {
      const end = Math.min(start + MESSAGES_PER_PAGE, messages.length);
      const nav = doc.createElement("nav");
      nav.className = "ui2-chats-pager";
      nav.setAttribute("aria-label", i18n.t("arch.messagesPagerLabel"));
      const go = (delta: number) => {
        pageIndex += delta;
        renderThread();
        const first = threadHost.querySelector<HTMLElement>(".ui2-chats-msg");
        if (first) {
          first.tabIndex = -1;
          first.focus();
          first.scrollIntoView?.({ block: "start" });
        }
      };
      const range = doc.createElement("span");
      range.className = "ui2-chats-range";
      range.setAttribute("role", "status");
      range.textContent = i18n.t("arch.messagesRange", {
        from: i18n.formatNumber(start + 1), to: i18n.formatNumber(end), total: i18n.formatNumber(messages.length),
      });
      nav.append(
        createUi2Button(doc, { label: i18n.t("arch.prevMessages"), size: "sm", disabled: pageIndex === 0, onClick: () => go(-1) }),
        range,
        createUi2Button(doc, { label: i18n.t("arch.nextMessages"), size: "sm", disabled: end >= messages.length, onClick: () => go(1) }),
      );
      threadHost.append(nav);
    }
  }

  function partialBanner(capture: Capture, summaryNow: ConversationSummary): HTMLElement | null {
    if (capture.completeness !== "partial" && summaryNow.completeness !== "partial") return null;
    const banner = doc.createElement("div");
    banner.className = "ui2-banner ui2-banner-warn";
    banner.setAttribute("role", "note");
    const text = doc.createElement("p");
    text.className = "ui2-banner-text";
    text.textContent = i18n.t("arch.partialBanner");
    const why = createUi2Button(doc, { label: i18n.t("arch.partialWhy"), variant: "ghost", size: "sm" });
    why.addEventListener("click", () => {
      infoAside?.open(why);
      infoBody?.completenessHeading.focus();
    });
    banner.append(text, why);
    return banner;
  }

  function render(): void {
    clearCopiedTimer();
    disposeAsides();
    root.replaceChildren();
    const bar = doc.createElement("div");
    bar.className = "ui2-chats-bar";
    const back = createUi2Button(doc, {
      label: i18n.t("arch.back"), ariaLabel: i18n.t("arch.back"), variant: "ghost", icon: "back", onClick: deps.onBack,
    });
    back.classList.add("ui2-chats-back");
    titleEl = doc.createElement("h1");
    titleEl.className = "ui2-chats-title";
    titleEl.tabIndex = -1;
    titleEl.textContent = title();
    bar.append(back, titleEl);
    statusLine = doc.createElement("p");
    statusLine.className = "ui2-chats-note ui2-chats-note-warn";
    statusLine.setAttribute("role", "status");
    copyButton = undefined;

    if (phase === "ready" && conversation) {
      infoAside = createUi2Aside(doc, { title: i18n.t("arch.info.title"), closeLabel: i18n.t("action.close") });
      infoAside.element.classList.add("ui2-chats-aside");
      exportPanel = createExportPanel({
        doc, i18n, invoke, clipboard: deps.clipboard,
        target: () => (conversation
          ? { id: summary.id, snapshotId: activeSnapshotId, title: title() }
          : null),
        onClose: () => exportPanel?.reset(),
      });
      exportPanel.aside.element.classList.add("ui2-chats-aside");

      const infoButton = createUi2Button(doc, { label: i18n.t("arch.info"), size: "sm", icon: "info" });
      infoButton.addEventListener("click", () => infoAside?.open(infoButton));
      copyButton = createUi2Button(doc, { label: i18n.t("action.copy"), size: "sm", icon: "copy" });
      copyButton.addEventListener("click", () => { void copyTranscript(); });
      const exportButton = createUi2Button(doc, { label: i18n.t("arch.export"), size: "sm", icon: "download" });
      exportButton.addEventListener("click", () => exportPanel?.aside.open(exportButton));
      bar.append(infoButton, copyButton, exportButton);

      root.append(bar, statusLine);
      renderInfo();
      const banner = partialBanner(conversation.capture, conversation.summary);
      if (banner) root.append(banner);
      renderThread();
      root.append(threadHost, infoAside.element, exportPanel.aside.element);
    } else if (phase === "error") {
      root.append(bar, createUi2EmptyState(doc, {
        icon: "info",
        title: i18n.t("arch.openErrorTitle"),
        body: i18n.t("arch.openErrorBody"),
        actionLabel: i18n.t("arch.back"),
        onAction: deps.onBack,
      }));
    } else {
      root.append(bar, createUi2LoadingSkeleton(doc, i18n.t("state.loading"), i18n));
    }
  }

  async function load(): Promise<void> {
    const version = ++loadVersion;
    phase = "loading";
    render();
    try {
      const [result, copies] = await Promise.all([
        invoke<Conversation>("read_conversation", { id: summary.id }),
        invoke<CaptureSnapshot[]>("list_snapshots", { id: summary.id }).catch(() => [] as CaptureSnapshot[]),
      ]);
      if (!alive || version !== loadVersion) return;
      if (result.summary.id !== summary.id) throw new Error("MP_UI2_CHAT_MISMATCH");
      conversation = result;
      snapshots = copies;
      activeSnapshotId = result.snapshot_id ?? copies[0]?.snapshot_id ?? null;
      pageIndex = 0;
      phase = "ready";
      render();
      deps.onTitle(title());
      titleEl?.focus();
    } catch {
      if (!alive || version !== loadVersion) return;
      phase = "error";
      render();
    }
  }

  void load();

  return {
    element: root,
    focusTitle: () => titleEl?.focus(),
    rebuild() { render(); },
    destroy() {
      alive = false;
      loadVersion += 1;
      renderGeneration += 1;
      clearCopiedTimer();
      disposeAsides();
      conversation = null;
      snapshots = [];
      root.replaceChildren();
      root.remove();
    },
  };
}

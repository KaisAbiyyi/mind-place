import createDOMPurify from "dompurify";
import { CONVERSATION_ROW_HEIGHT, VIRTUAL_LIST_THRESHOLD, WORKSPACES, bindWorkspaceNavigationKeyboard, createWorkspaceNavigation, getVirtualWindow, setActiveRoute, type WorkspaceId } from "./knowledge-os.js";
import { isUiRouteId, routeForWorkspace, type UiCommandRegistration, type UiRouteId } from "./ui-contracts.js";
import { createUiButton, createUiCard, createUiStatePanel, createUiStatus, createUiTable } from "./ui-primitives.js";
import { APPLICATION_SHELL_MARKUP, mountUiShell } from "./ui-shell.js";
import { mountLocalMcpSetup } from "./mcp-setup.js";
import { mountDocumentRootSetup } from "./document-roots.js";
import { mountFilesVaultWorkspace, type FilesVaultHandle } from "./files-vault-workspace.js";
import { mountMemoryInspector, type MemoryInspectorHandle } from "./memory-inspector.js";
import { mountHiveWorkspace, type HiveHandle } from "./hive-workspace.js";
import { mountLibraryWorkspace, type LibraryHandle } from "./library-workspace.js";
import { mountHealthWorkspace, type HealthWorkspaceHandle } from "./health-workspace.js";
import { mountSettingsWorkspace, type SettingsWorkspaceHandle } from "./settings-workspace.js";
import {
  createBrowserProgressStore, hasFinishedOnboarding, mountOnboardingWorkspace,
  type OnboardingProgressStore, type OnboardingWorkspaceHandle,
} from "./onboarding-workspace.js";
import { marked } from "marked";
import renderMathInElement from "katex/contrib/auto-render";
import type {
  AssetResult,
  Attachment,
  CaptureSnapshot,
  Conversation,
  ConversationSummary,
  ExportResult,
  LegacyImportResult,
  GenericImportReceipt,
  Message,
  SetupInfo,
} from "./contracts.js";
import { createArchiveFilters, createConversationArchive } from "./conversation-archive.js";
import { mountProvidersWorkspace, type ProvidersHandle } from "./providers-workspace.js";

export type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

export interface ClipboardWriter {
  writeText(value: string): Promise<void>;
}

export interface ReaderOptions {
  root: HTMLElement;
  invoke: Invoke;
  clipboard?: ClipboardWriter;
  onLock?: () => void;
  /** Set only for the session that created the unlock secret; opens Setup & Onboarding once. */
  openOnboarding?: boolean;
  /** Non-sensitive wizard progress (step and skip/finish markers only). */
  onboardingStore?: OnboardingProgressStore;
}

export interface ReaderHandle {
  ready: Promise<void>;
  refresh(): Promise<void>;
  destroy(): void;
}

const ALLOWED_IMAGE_MIME_TYPES = new Set([
  "image/avif",
  "image/gif",
  "image/jpeg",
  "image/png",
  "image/webp",
]);

/** Render transcript text as inert, sanitized DOM. Attachments are handled separately via get_asset. */
export function renderMessageContent(document: Document, message: Pick<Message, "text" | "format">): HTMLElement {
  const content = document.createElement("div");
  content.className = "message-content";

  if (message.format === "plain") {
    const paragraph = document.createElement("p");
    paragraph.textContent = message.text;
    content.append(paragraph);
    return content;
  }

  const html = marked.parse(message.text, { async: false, gfm: true });
  const browserWindow = document.defaultView;
  if (!browserWindow) throw new Error("Transcript rendering requires a browser document.");
  const purifier = createDOMPurify(browserWindow);
  const safeHtml = purifier.sanitize(typeof html === "string" ? html : "", {
    ALLOWED_TAGS: [
      "a", "blockquote", "br", "code", "del", "div", "em", "h1", "h2", "h3", "h4", "h5", "h6",
      "hr", "li", "ol", "p", "pre", "strong", "sub", "sup", "table", "tbody", "td", "th", "thead",
      "tr", "ul", "span",
    ],
    ALLOWED_ATTR: ["class", "colspan", "href", "rowspan", "start", "title"],
    FORBID_ATTR: ["style"],
    FORBID_TAGS: ["audio", "embed", "iframe", "link", "math", "object", "picture", "script", "source", "style", "svg", "track", "video"],
  });

  const template = document.createElement("template");
  template.innerHTML = safeHtml;
  content.append(template.content.cloneNode(true));

  for (const link of content.querySelectorAll<HTMLAnchorElement>("a[href]")) {
    const rawHref = link.getAttribute("href") ?? "";
    let parsed: URL | undefined;
    try {
      parsed = new URL(rawHref);
    } catch {
      // Relative and malformed destinations do not have a useful archive target.
    }
    if (!parsed || !["http:", "https:", "mailto:"].includes(parsed.protocol)) {
      link.removeAttribute("href");
      continue;
    }
    link.href = parsed.href;
    if (parsed.protocol === "http:" || parsed.protocol === "https:") {
      link.target = "_blank";
      link.rel = "noopener noreferrer";
    }
  }

  try {
    renderMathInElement(content, {
      delimiters: [
        { left: "$$", right: "$$", display: true },
        { left: "\\[", right: "\\]", display: true },
        { left: "\\(", right: "\\)", display: false },
        { left: "$", right: "$", display: false },
      ],
      throwOnError: false,
      strict: "ignore",
    });
  } catch {
    // Keep the original sanitized text if a malformed formula cannot be typeset.
  }

  return content;
}

export function mountReader({ root, invoke: nativeInvoke, clipboard, onLock, openOnboarding, onboardingStore }: ReaderOptions): ReaderHandle {
  const document = root.ownerDocument;
  const clipboardWriter = clipboard ?? {
    writeText: (value: string) => document.defaultView?.navigator.clipboard?.writeText(value)
      ?? Promise.reject(new Error("Clipboard access is unavailable in this app window.")),
  };

  root.innerHTML = APPLICATION_SHELL_MARKUP;
  root.querySelector("#knowledge-os-nav-slot")?.replaceWith(createWorkspaceNavigation(document));
  const uiShell = mountUiShell(root, { onNavigate: navigateToRoute, onCommand: executeShellCommand });
  let summaries: ConversationSummary[] = [];
  let selectedId: string | null = null;
  let selectionVersion = 0;
  let listVersion = 0;
  let setupVersion = 0;
  let activeSettingsTab: "archive" | "documents" | "connections" | "privacy" = "archive";
  let loadedConversation: Conversation | null = null;
  let loadedSnapshots: CaptureSnapshot[] = [];
  let selectedSnapshotId: string | null = null;
  let exportedMarkdown = "";
  let memoryInspector: MemoryInspectorHandle | undefined;
  let healthWorkspace: HealthWorkspaceHandle | undefined;
  let settingsWorkspace: SettingsWorkspaceHandle | undefined;
  let libraryWorkspace: LibraryHandle | undefined;
  let hiveWorkspace: HiveHandle | undefined;
  let providersWorkspace: ProvidersHandle | undefined;
  let setupMcp: { destroy(): void } | undefined;
  let filesVaultWorkspace: FilesVaultHandle | undefined;
  let onboardingWorkspace: OnboardingWorkspaceHandle | undefined;
  const progressStore = onboardingStore ?? createBrowserProgressStore(document.defaultView);
  let alive = true;
  const invoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
    if (!alive) throw new Error("MP_UI_SESSION_EXPIRED");
    const result = await nativeInvoke<T>(command, args);
    if (!alive) throw new Error("MP_UI_SESSION_EXPIRED");
    return result;
  };
  const safeClipboard: ClipboardWriter = {
    writeText: (value) => alive
      ? clipboardWriter.writeText(value)
      : Promise.reject(new Error("MP_UI_SESSION_EXPIRED")),
  };
  for (const button of root.querySelectorAll<HTMLButtonElement>("[data-action='lock']")) {
    button.hidden = !onLock;
  }

  const get = <T extends HTMLElement>(selector: string): T => {
    const element = root.querySelector<T>(selector);
    if (!element) throw new Error(`Reader surface is missing ${selector}`);
    return element;
  };

  const list = get<HTMLDivElement>("#conversation-list");
  const workspaceNav = get<HTMLElement>("#knowledge-os-nav");
  const disposeWorkspaceKeys = bindWorkspaceNavigationKeyboard(workspaceNav);
  const view = get<HTMLElement>("#reader-view");
  let activeWorkspace: WorkspaceId = "conversations";
  let activeRoute: UiRouteId = "overview";
  let modalReturnWorkspace: WorkspaceId = "conversations";
  let modalReturnRoute: UiRouteId = "overview";
  let modalReturnFocus: HTMLElement | null = null;
  let archiveLoadState: "loading" | "ready" | "error" = "loading";
  const archiveFilters = createArchiveFilters();
  let archiveTableVisible = false;
  let transcriptPage = 0;
  const activateRoute = (route: UiRouteId, workspace: WorkspaceId = activeWorkspace) => {
    if (route !== "files-secure-vault" && filesVaultWorkspace) {
      filesVaultWorkspace.destroy(); filesVaultWorkspace = undefined;
    }
    if (route !== "ai-providers-mcp" && providersWorkspace) { providersWorkspace.destroy(); providersWorkspace = undefined; }
    if ((route !== "settings" || workspace !== "settings") && settingsWorkspace) { settingsWorkspace.destroy(); settingsWorkspace = undefined; }
    // Leaving Setup & Onboarding unmounts the wizard and fences its late async work.
    if (route !== "setup-onboarding" && onboardingWorkspace) { onboardingWorkspace.destroy(); onboardingWorkspace = undefined; }
    activeWorkspace = workspace;
    activeRoute = route;
    setActiveRoute(workspaceNav, route, workspace);
    uiShell.setActiveRoute(route, workspace);
    const label = WORKSPACES.find(item => item.id === workspace)?.label ?? "Archive";
    view.setAttribute("aria-label", label + " workspace");
  };
  const activateWorkspace = (id: WorkspaceId) => {
    activeWorkspace = id;
    activateRoute(routeForWorkspace(id), id);
  };
  const focusViewHeading = () => {
    view.scrollTop = 0;
    const heading = view.querySelector<HTMLElement>("h1, .state-panel h2, .ui-state-panel h2");
    if (heading) {
      heading.tabIndex = -1;
      heading.focus({ preventScroll: true });
    }
  };
  activateRoute("overview", "conversations");
  const search = get<HTMLInputElement>("#archive-search");
  const dialog = get<HTMLDialogElement>("#setup-dialog");
  const setupContent = get<HTMLDivElement>("#setup-content");
  const status = get<HTMLSpanElement>("#action-status");
  const toolbarTitle = get<HTMLSpanElement>(".toolbar-context span:last-child");
  const copyButton = get<HTMLButtonElement>("[data-action='copy-answer']");
  const exportButton = get<HTMLButtonElement>("[data-action='export-markdown']");

  const setActionStatus = (text: string, error = false) => {
    status.textContent = text;
    status.classList.toggle("is-error", error);
  };

  const setViewBusy = (busy: boolean) => view.setAttribute("aria-busy", String(busy));

  const renderListMessage = (text: string, error = false) => {
    list.classList.remove("is-virtualized");
    list.replaceChildren();
    const paragraph = document.createElement("p");
    paragraph.className = error ? "list-note list-error" : "list-note";
    paragraph.textContent = text;
    list.append(paragraph);
    if (error) {
      const retry = document.createElement("button");
      retry.className = "text-button list-retry";
      retry.type = "button";
      retry.dataset.action = "refresh";
      retry.textContent = "Try again";
      list.append(retry);
    }
  };

  const renderState = (title: string, detail: string, variant: "empty" | "loading" | "error" = "empty") => {
    setViewBusy(variant === "loading");
    if (variant === "loading") {
      copyButton.disabled = true;
      exportButton.disabled = true;
    }
    view.replaceChildren();
    const panel = document.createElement("div");
    panel.className = `state-panel ${variant}-panel`;
    if (variant === "loading") {
      const mark = document.createElement("span");
      mark.className = "loading-mark";
      mark.setAttribute("aria-hidden", "true");
      panel.append(mark);
    }
    const heading = document.createElement("h2");
    heading.textContent = title;
    const paragraph = document.createElement("p");
    paragraph.textContent = detail;
    panel.append(heading, paragraph);
    if (variant === "error") {
      const retry = document.createElement("button");
      retry.className = "primary-button state-retry";
      retry.type = "button";
      retry.dataset.action = selectedId ? "retry-selection" : "refresh";
      retry.textContent = selectedId ? "Try again" : "Refresh archive";
      panel.append(retry);
    }
    view.append(panel);
  };

  const updateToolbarSelection = (summary?: ConversationSummary) => {
    if (activeRoute === "overview" && activeWorkspace === "conversations") toolbarTitle.textContent = "Overview";
    else if (activeWorkspace === "library") toolbarTitle.textContent = "Library search";
    else if (activeRoute === "conversations") toolbarTitle.textContent = summary ? "Conversation reader" : "Your archive";
    copyButton.disabled = !summary;
    exportButton.disabled = !summary;
  };

  const renderOverview = () => {
    if (archiveLoadState === "loading") {
      setViewBusy(true);
      updateToolbarSelection();
      view.replaceChildren(createUiStatePanel(document, {
        state: "loading",
        title: "Loading your archive",
        detail: "Reading the saved conversation index from this device.",
      }));
      return;
    }
    if (archiveLoadState === "error") {
      setViewBusy(false);
      updateToolbarSelection();
      const panel = createUiStatePanel(document, {
        state: "error",
        title: "Archive unavailable",
        detail: "The local archive could not be read. Check the archive service and retry.",
      });
      const retry = createUiButton(document, "Refresh archive", { variant: "secondary" });
      retry.dataset.action = "refresh";
      panel.append(retry);
      view.replaceChildren(panel);
      return;
    }

    const total = summaries.length;
    const messageCount = summaries.reduce((sum, item) => sum + item.message_count, 0);
    const completeCount = summaries.filter((item) => item.completeness === "complete").length;
    const partialCount = summaries.filter((item) => item.completeness === "partial").length;
    const page = document.createElement("div");
    page.className = "ui-overview-page";
    const heading = document.createElement("header");
    heading.className = "ui-overview-heading";
    const title = document.createElement("h1");
    title.textContent = "Overview";
    const description = document.createElement("p");
    description.textContent = "A live view of the conversations saved in your local archive.";
    heading.append(title, description);
    page.append(heading);

    const metrics = document.createElement("div");
    metrics.className = "ui-overview-grid";
    const metricValues = [
      { label: "Saved conversations", value: total, detail: "Available in this archive" },
      { label: "Messages captured", value: messageCount, detail: "Across saved conversations" },
      { label: "Complete captures", value: completeCount, detail: "Marked complete by capture metadata" },
      { label: "Partial captures", value: partialCount, detail: "Contain capture warnings or gaps" },
    ];
    for (const metric of metricValues) {
      const card = createUiCard(document, { title: metric.label, className: "ui-stat-card" });
      const value = document.createElement("strong");
      value.className = "ui-stat-value";
      value.textContent = metric.value.toLocaleString();
      const detail = document.createElement("span");
      detail.className = "ui-stat-detail";
      detail.textContent = metric.detail;
      card.body.append(value, detail);
      metrics.append(card.element);
    }
    page.append(metrics);

    const lower = document.createElement("div");
    lower.className = "ui-overview-lower";
    const recentCard = createUiCard(document, {
      title: "Recent conversations",
      description: "Open a saved conversation to inspect its captured transcript.",
    });
    const recent = [...summaries]
      .sort((left, right) => new Date(right.captured_at).getTime() - new Date(left.captured_at).getTime())
      .slice(0, 6);
    const rows = recent.map((item) => {
      const link = createUiButton(document, item.title || "Untitled conversation", { variant: "link", className: "ui-table-link" });
      link.dataset.conversationId = item.id;
      const completeness = item.completeness === "complete" ? "complete" : item.completeness === "partial" ? "partial" : "neutral";
      return [
        link,
        item.message_count.toLocaleString(),
        formatRelativeDate(item.conversation_date || item.captured_at),
        createUiStatus(document, item.completeness === "partial" ? "Partial" : "Complete", completeness),
      ];
    });
    recentCard.body.append(createUiTable(document, {
      caption: "Most recently captured conversations",
      columns: ["Conversation", "Messages", "Date", "Capture"],
      rows,
      emptyLabel: "No saved conversations yet. Save a conversation with the browser extension to see it here.",
    }));
    lower.append(recentCard.element);

    const actionsCard = createUiCard(document, {
      title: "Quick access",
      description: "Jump to an existing Mind Place workspace.",
    });
    const actions = document.createElement("div");
    actions.className = "ui-overview-actions";
    const destinations: Array<{ label: string; route: UiRouteId }> = [
      { label: "Conversations", route: "conversations" },
      { label: "Curated Memory", route: "curated-memory" },
      { label: "Hive Mind Graph", route: "hive-mind-graph" },
      { label: "Files & Secure Vault", route: "files-secure-vault" },
      { label: "AI Providers & MCP", route: "ai-providers-mcp" },
    ];
    for (const destination of destinations) {
      const button = createUiButton(document, destination.label, { variant: "secondary" });
      button.dataset.action = "navigate-route";
      button.dataset.route = destination.route;
      actions.append(button);
    }
    const sourceSearch = createUiButton(document, "Search all sources", { variant: "quiet" });
    sourceSearch.dataset.action = "open-library-workspace";
    actions.append(sourceSearch);
    actionsCard.body.append(actions);
    lower.append(actionsCard.element);
    page.append(lower);

    if (total === 0) {
      page.append(createUiStatePanel(document, {
        state: "empty",
        title: "Your archive is empty",
        detail: "Save a conversation with the browser extension. Saved chats remain on this device.",
      }));
    }
    view.replaceChildren(page);
    setViewBusy(false);
    updateToolbarSelection();
  };

  const renderConversationRows = (query: string) => {
    const focusId = (document.activeElement as HTMLElement | null)?.dataset?.conversationId;
    const virtual = summaries.length > VIRTUAL_LIST_THRESHOLD;
    const previousScroll = list.scrollTop;
    const range = virtual
      ? getVirtualWindow(summaries.length, previousScroll, list.clientHeight, CONVERSATION_ROW_HEIGHT)
      : { start: 0, end: summaries.length, beforePx: 0, afterPx: 0 };
    const fragment = document.createDocumentFragment();
    const spacer = (height: number) => {
      const element = document.createElement("div");
      element.className = "knowledge-list-spacer";
      element.setAttribute("aria-hidden", "true");
      element.style.height = `${height}px`;
      return element;
    };
    if (virtual && range.beforePx) fragment.append(spacer(range.beforePx));
    for (let i = range.start; i < range.end; i++) {
      const item = summaries[i];
      const row = document.createElement("button");
      row.type = "button";
      row.className = "conversation-row";
      row.dataset.conversationId = item.id;
      row.dataset.rowIndex = String(i);
      row.setAttribute("role", "listitem");
      row.setAttribute("aria-setsize", String(summaries.length));
      row.setAttribute("aria-posinset", String(i + 1));
      row.setAttribute("aria-pressed", String(item.id === selectedId));
      const title = document.createElement("span");
      title.className = "conversation-title";
      title.textContent = item.title || "Untitled conversation";
      const meta = document.createElement("span");
      meta.className = "conversation-meta";
      const captureDateFallback = item.provider === "gemini" && !item.conversation_date;
      meta.textContent = captureDateFallback
        ? `Captured ${formatDate(item.captured_at)} · ${item.message_count} messages`
        : `${formatRelativeDate(item.conversation_date)} · ${item.message_count} messages`;
      row.append(title, meta);
      if (item.completeness === "partial") {
        const marker = document.createElement("span");
        marker.className = "partial-marker";
        marker.textContent = "Partial capture";
        row.append(marker);
      }
      fragment.append(row);
    }
    if (virtual && range.afterPx) fragment.append(spacer(range.afterPx));
    list.classList.toggle("is-virtualized", virtual);
    list.replaceChildren(fragment);
    if (virtual && list.scrollTop !== previousScroll) list.scrollTop = previousScroll;
    // Replacing the visible window must not strand keyboard focus when its row stays mounted.
    if (focusId && document.activeElement === document.body) {
      for (const row of list.querySelectorAll<HTMLButtonElement>("[data-conversation-id]")) {
        if (row.dataset.conversationId === focusId) {
          row.focus({ preventScroll: true });
          break;
        }
      }
    }

    if (summaries.length === 0) {
      renderListMessage(query ? "No conversations match this search." : "No saved conversations yet.");
      if (!selectedId && activeRoute === "conversations") {
        renderState(
          query ? "No matches" : "Your archive is empty",
          query ? "Try another title or phrase." : "Save a conversation with the browser extension. Your saved chats stay on this device.",
        );
      }
    }
  };

  const selectConversation = async (id: string) => {
    archiveTableVisible = false;
    transcriptPage = 0;
    providersWorkspace?.destroy(); providersWorkspace = undefined;
    hiveWorkspace?.destroy(); hiveWorkspace = undefined;
    libraryWorkspace?.destroy(); libraryWorkspace = undefined;
    healthWorkspace?.destroy();
    healthWorkspace = undefined;
    settingsWorkspace?.destroy();
    settingsWorkspace = undefined;
    memoryInspector?.destroy();
    memoryInspector = undefined;
    selectedId = id;
    activateWorkspace("conversations");
    loadedConversation = null;
    loadedSnapshots = [];
    selectedSnapshotId = null;
    exportedMarkdown = "";
    const version = ++selectionVersion;
    const summary = summaries.find((item) => item.id === id);
    renderConversationRows(search.value.trim());
    updateToolbarSelection(summary);
    setActionStatus("");
    renderState("Opening conversation", "Loading the saved transcript…", "loading");

    try {
      const [result, snapshots] = await Promise.all([
        invoke<Conversation>("read_conversation", { id }),
        invoke<CaptureSnapshot[]>("list_snapshots", { id }).catch(() => [] as CaptureSnapshot[]),
      ]);
      if (!alive || version !== selectionVersion || selectedId !== id) return;
      if (result.summary.id !== id) throw new Error("The archive returned a different conversation.");
      loadedConversation = result;
      loadedSnapshots = snapshots;
      selectedSnapshotId = result.snapshot_id ?? snapshots[0]?.snapshot_id ?? null;
      renderConversation(result, version, snapshots, selectedSnapshotId);
    } catch (error) {
      if (!alive || version !== selectionVersion || selectedId !== id) return;
      renderState("Couldn’t open this conversation", errorMessage(error), "error");
    }
  };

  const loadSnapshot = async (snapshotId: string) => {
    if (!selectedId || !snapshotId || snapshotId === selectedSnapshotId) return;
    transcriptPage = 0;
    const id = selectedId;
    const version = ++selectionVersion;
    exportedMarkdown = "";
    renderState("Opening saved capture", "Loading this conversation snapshot…", "loading");
    try {
      const result = await invoke<Conversation>("read_conversation", { id, snapshotId });
      if (!alive || version !== selectionVersion || selectedId !== id) return;
      if (result.summary.id !== id) throw new Error("The archive returned a different conversation.");
      loadedConversation = result;
      selectedSnapshotId = snapshotId;
      renderConversation(result, version, loadedSnapshots, selectedSnapshotId);
    } catch (error) {
      if (!alive || version !== selectionVersion || selectedId !== id) return;
      renderState("Couldn’t open this capture", errorMessage(error), "error");
    }
  };

  const loadAsset = async (
    conversationId: string,
    attachment: Attachment,
    imageSlot: HTMLElement,
    version: number,
  ) => {
    if (!attachment.asset_path) return;
    try {
      const asset = await invoke<AssetResult>("get_asset", { id: conversationId, assetPath: attachment.asset_path });
      if (!alive || version !== selectionVersion || selectedId !== conversationId || !imageSlot.isConnected) return;
      const mimeType = asset.mime_type.toLowerCase();
      if (!ALLOWED_IMAGE_MIME_TYPES.has(mimeType) || !isSafeImageDataUrl(asset.data_url, mimeType)) {
        throw new Error("This captured image has an unsupported format.");
      }
      const image = document.createElement("img");
      image.className = "captured-image";
      image.alt = attachment.name || "Captured image";
      image.decoding = "async";
      image.src = asset.data_url;
      imageSlot.replaceChildren(image);
      imageSlot.classList.remove("image-slot-loading");
    } catch (error) {
      if (!alive || version !== selectionVersion || selectedId !== conversationId || !imageSlot.isConnected) return;
      imageSlot.classList.remove("image-slot-loading");
      imageSlot.textContent = errorMessage(error);
      imageSlot.classList.add("image-slot-error");
    }
  };

  const renderAttachment = (conversationId: string, attachment: Attachment, version: number): HTMLElement => {
    const item = document.createElement("div");
    item.className = "attachment-item";
    if (attachment.mime_type.toLowerCase().startsWith("image/")) {
      const slot = document.createElement("div");
      slot.className = "image-slot";
      if (attachment.asset_path) {
        slot.classList.add("image-slot-loading");
        slot.textContent = "Loading captured image…";
        void loadAsset(conversationId, attachment, slot, version);
      } else {
        slot.classList.add("image-slot-unavailable");
        slot.textContent = attachment.unavailable_reason || "This image is not available in the local archive.";
      }
      item.append(slot);
      const caption = document.createElement("p");
      caption.className = "attachment-caption";
      caption.textContent = attachment.name || "Captured image";
      item.append(caption);
      return item;
    }

    const file = document.createElement("div");
    file.className = "file-reference";
    const name = document.createElement("span");
    name.className = "file-name";
    name.textContent = attachment.name || "Attached file";
    const detail = document.createElement("span");
    detail.className = "file-detail";
    detail.textContent = attachment.unavailable_reason || attachment.mime_type;
    file.append(name, detail);
    item.append(file);
    return item;
  };

  function renderConversation(
    result: Conversation,
    version: number,
    snapshots: CaptureSnapshot[] = [],
    activeSnapshotId: string | null = null,
  ) {
    const summary = result.summary;
    const capture = result.capture;
    const top = document.createElement("div");
    top.className = "conversation-heading";
    const eyebrow = document.createElement("p");
    eyebrow.className = "eyebrow conversation-eyebrow";
    eyebrow.textContent = "Saved conversation";
    const heading = document.createElement("h1");
    heading.className = "conversation-heading-title";
    heading.textContent = capture.title || summary.title || "Untitled conversation";
    const metadata = document.createElement("div");
    metadata.className = "conversation-details";
    const originalDate = document.createElement("span");
    const originalDateLabel = summary.date_source === "provider-updated"
      ? "Updated"
      : summary.date_source === "message-authored"
        ? "Last message"
        : summary.date_source === "provider-created"
          ? "Created"
          : "Conversation date";
    originalDate.textContent = summary.conversation_date
      ? `${originalDateLabel} ${formatDate(summary.conversation_date)}`
      : "Date unavailable";
    // Gemini without a provider date already has an honest capture timestamp;
    // avoid showing an unrelated "Date unavailable" beside that timestamp.
    const metadataParts: Node[] = summary.provider === "gemini" && !summary.conversation_date
      ? [] : [originalDate];
    if (capture.conversation_created_at && summary.date_source !== "provider-created") {
      const created = document.createElement("span");
      created.textContent = `Created ${formatDate(capture.conversation_created_at)}`;
      if (metadataParts.length) metadataParts.push(document.createTextNode(" · "));
      metadataParts.push(created);
    }
    const date = document.createElement("span");
    date.textContent = `Captured ${formatDate(capture.captured_at)}`;
    const messageCount = document.createElement("span");
    messageCount.textContent = `${capture.messages.length} messages in this snapshot`;
    const captureOrigin = document.createElement("span");
    captureOrigin.textContent = capture.capture_method === "legacy-import" ? "Imported legacy archive"
      : capture.capture_method === "automatic-dom" ? "Automatic browser save · loaded messages only" : "Manual browser save";
    const origin = makeSourceLink(document, capture.source_url || summary.source_url);
    if (metadataParts.length) metadataParts.push(document.createTextNode(" · "));
    metadataParts.push(
      date,
      document.createTextNode(" · "), messageCount,
      document.createTextNode(" · "), captureOrigin,
      document.createTextNode(" · "), origin,
    );
    metadata.append(...metadataParts);
    top.append(eyebrow, heading, metadata);
    const detailActions = document.createElement("div"); detailActions.className = "conversation-detail-actions";
    const back = createUiButton(document, "Back to Conversations"); back.dataset.action = "archive-table";
    const copyTranscript = createUiButton(document, "Copy transcript"); copyTranscript.dataset.action = "copy-transcript";
    detailActions.append(back, copyTranscript); top.append(detailActions);
    if (snapshots.length > 1) {
      const history = document.createElement("div");
      history.className = "capture-history";
      const historyLabel = document.createElement("label");
      historyLabel.htmlFor = "capture-snapshot";
      historyLabel.textContent = "Saved capture";
      const select = document.createElement("select");
      select.id = "capture-snapshot";
      select.dataset.action = "select-snapshot";
      select.setAttribute("aria-label", "Choose a saved capture");
      for (const snapshot of snapshots) {
        const option = document.createElement("option");
        option.value = snapshot.snapshot_id;
        const completeness = snapshot.completeness === "partial" ? " · Partial" : "";
        option.textContent = `${formatDate(snapshot.captured_at)} · ${snapshot.message_count} messages${completeness}`;
        option.selected = snapshot.snapshot_id === activeSnapshotId;
        select.append(option);
      }
      history.append(historyLabel, select);
      top.append(history);
    }

    const transcript = document.createElement("div");
    transcript.className = "transcript";
    if (capture.completeness === "partial" || summary.completeness === "partial") {
      const warning = document.createElement("aside");
      warning.className = "capture-warning";
      warning.setAttribute("role", "note");
      const warningTitle = document.createElement("strong");
      warningTitle.textContent = "Partial capture";
      const warningText = document.createElement("p");
      warningText.textContent = "The source page did not expose a complete transcript when this snapshot was saved. Some earlier or later messages may be missing.";
      warning.append(warningTitle, warningText);
      transcript.append(warning);
    }
    if (capture.warnings.length) {
      const warningList = document.createElement("ul");
      warningList.className = "capture-notes";
      for (const warningText of capture.warnings) {
        const row = document.createElement("li");
        row.textContent = warningText;
        warningList.append(row);
      }
      transcript.append(warningList);
    }

    const messageList = document.createElement("div");
    messageList.className = "message-list";
    const messageStart = transcriptPage * 40;
    for (const [index, message] of capture.messages.slice(messageStart, messageStart + 40).entries()) {
      const card = document.createElement("article");
      card.className = `message message-${message.role}`;
      card.dataset.messageId = message.id;
      const messageHeader = document.createElement("header");
      messageHeader.className = "message-header";
      const role = document.createElement("h2");
      role.className = "message-role";
      role.textContent = message.role === "assistant" ? "Assistant" : message.role === "user" ? "You" : titleCase(message.role);
      messageHeader.append(role);
      const detail = document.createElement("span");
      detail.className = "message-detail";
      const detailParts = [message.model, message.authored_at ? formatTime(message.authored_at) : ""].filter(Boolean);
      detail.textContent = detailParts.join(" · ");
      if (detail.textContent) messageHeader.append(detail);
      if (message.role === "assistant") {
        const copy = document.createElement("button");
        copy.type = "button";
        copy.className = "message-copy";
        copy.dataset.action = "copy-message";
        copy.dataset.messageId = message.id;
        copy.textContent = "Copy answer";
        copy.setAttribute("aria-label", `Copy answer at message ${messageStart + index + 1}`);
        messageHeader.append(copy);
      }

      const body = renderMessageContent(document, message);
      card.append(messageHeader, body);
      for (const attachment of message.attachments) card.append(renderAttachment(summary.id, attachment, version));
      messageList.append(card);
    }
    if (capture.messages.length === 0) {
      const empty = document.createElement("p");
      empty.className = "transcript-empty";
      empty.textContent = "This snapshot contains no readable messages.";
      messageList.append(empty);
    }
    transcript.append(messageList);
    if (capture.messages.length > 40) {
      const pagination = document.createElement("nav"); pagination.className = "transcript-pagination";
      pagination.setAttribute("aria-label", "Transcript messages");
      const previous = createUiButton(document, "Previous messages", { disabled: transcriptPage === 0 }); previous.dataset.action = "transcript-previous";
      const range = document.createElement("span"); range.setAttribute("role", "status");
      range.textContent = `Messages ${messageStart + 1}–${Math.min(messageStart + 40, capture.messages.length)} of ${capture.messages.length}`;
      const next = createUiButton(document, "Next messages", { disabled: messageStart + 40 >= capture.messages.length }); next.dataset.action = "transcript-next";
      pagination.append(previous, range, next); transcript.append(pagination);
    }
    const provenance = document.createElement("aside"); provenance.className = "conversation-provenance";
    const provenanceTitle = document.createElement("h2"); provenanceTitle.textContent = "Provenance & Metadata";
    const fields = document.createElement("dl");
    for (const [label, value] of [["Provider", summary.provider], ["Conversation ID", capture.conversation_id],
      ["Archive ID", summary.id], ["Saved revision", activeSnapshotId ?? "Not supplied by archive"], ["Captured", capture.captured_at],
      ["Capture method", capture.capture_method], ["Completeness", capture.completeness]]) {
      const term = document.createElement("dt"); term.textContent = label;
      const definition = document.createElement("dd"); definition.textContent = value; fields.append(term, definition);
    }
    const branches = document.createElement("p"); branches.textContent = "Saved captures preserve the branches exposed by the source. A separate branch map is not supplied by this archive.";
    const related = document.createElement("p"); related.textContent = "Related memories are not supplied by this archive response. Inspect global claims in Curated Memory.";
    const memories = createUiButton(document, "Open Curated Memory"); memories.dataset.action = "open-memory-inspector";
    provenance.append(provenanceTitle, fields, branches, related, memories);
    const grid = document.createElement("div"); grid.className = "conversation-detail-grid"; grid.append(transcript, provenance);
    view.replaceChildren(top, grid);
    setViewBusy(false);
    updateToolbarSelection(summary);
  }

  const selectSettingsTab = (tab: typeof activeSettingsTab, focusTab = false) => {
    activeSettingsTab = tab;
    for (const button of root.querySelectorAll<HTMLButtonElement>("[data-settings-tab]")) {
      const selected = button.dataset.settingsTab === tab;
      button.setAttribute("aria-selected", String(selected));
      button.tabIndex = selected ? 0 : -1;
      if (selected && focusTab) button.focus();
    }
    for (const panel of setupContent.querySelectorAll<HTMLElement>("[data-settings-panel]")) {
      panel.hidden = panel.dataset.settingsPanel !== tab;
    }
  };

  const renderSetup = (info: SetupInfo) => {
    setupMcp?.destroy(); setupMcp = undefined;
    setupContent.replaceChildren();
    const makePanel = (tab: typeof activeSettingsTab, title: string, description: string) => {
      const panel = document.createElement("section");
      panel.className = "settings-panel";
      panel.id = `settings-panel-${tab}`;
      panel.dataset.settingsPanel = tab;
      panel.setAttribute("role", "tabpanel");
      panel.setAttribute("aria-labelledby", `settings-tab-${tab}`);
      const heading = document.createElement("h3");
      heading.className = "settings-page-title";
      heading.textContent = title;
      const note = document.createElement("p");
      note.className = "settings-page-description";
      note.textContent = description;
      panel.append(heading, note);
      return panel;
    };
    const archivePanel = makePanel("archive", "Archive & Capture", "Configure browser capture and find the locally stored archive.");
    const documentPanel = makePanel("documents", "Documents & Permissions", "Approve only folders you choose. No filesystem read occurs when you stage a path.");
    const connectionPanel = makePanel("connections", "Connections & MCP", "Local AI clients and hosted access remain stopped until you explicitly start them.");
    const privacyPanel = makePanel("privacy", "Privacy & Security", "Mind Place starts locked. Folder approval and remote connections require owner consent.");
    const connection = document.createElement("p");
    connection.className = "setup-intro";
    connection.textContent = `The reader stores archives locally. Register the extension once to connect manual saves from ${info.platform || "this device"}.`;

    const pathList = document.createElement("dl");
    pathList.className = "setup-paths";
    for (const [label, key] of [
      ["Archive folder", "archive_path"],
      ["Extension folder", "extension_path"],
      ["Native host", "host_path"],
      ["Extension ID", "extension_id"],
      ["Native host name", "native_host_name"],
    ] as const) {
      const term = document.createElement("dt");
      term.textContent = label;
      const value = document.createElement("dd");
      const code = document.createElement("code");
      code.textContent = info[key] || "Unavailable";
      value.append(code);
      pathList.append(term, value);
    }
    const registered = document.createElement("p");
    registered.className = "registered-status";
    registered.textContent = info.registered_browsers.length
      ? `Configured for ${info.registered_browsers.map(titleCase).join(" and ")}; browser connection has not been tested.`
      : "The browser extension has not been registered yet.";

    const register = document.createElement("div");
    register.className = "register-actions";
    for (const browser of ["chrome", "edge", "brave"] as const) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "quiet-button";
      button.dataset.action = `register-${browser}`;
      button.textContent = `Register ${titleCase(browser)}`;
      register.append(button);
    }

    const divider = document.createElement("hr");
    const legacyTitle = document.createElement("h3");
    legacyTitle.className = "setup-section-title";
    legacyTitle.textContent = "Import a legacy archive";
    const legacyText = document.createElement("p");
    legacyText.textContent = info.legacy_available
      ? "Copy saved conversations from the previous Mind Place archive format. The original archive stays in place."
      : "No compatible legacy archive was found on this device.";
    const legacyButton = document.createElement("button");
    legacyButton.type = "button";
    legacyButton.className = "primary-button import-button";
    legacyButton.dataset.action = "import-legacy";
    legacyButton.textContent = "Import legacy archive";
    legacyButton.disabled = !info.legacy_available;
    const importStatus = document.createElement("div");
    importStatus.className = "import-status";
    importStatus.dataset.setupStatus = "import";
    importStatus.setAttribute("role", "status");
    importStatus.setAttribute("aria-live", "polite");

    archivePanel.append(connection, pathList, registered, register, divider, legacyTitle, legacyText, legacyButton, importStatus);
    const genericTitle = document.createElement("h3");
    genericTitle.className = "setup-section-title";
    genericTitle.textContent = "Import a generic transcript";
    const genericInfo = document.createElement("p");
    genericInfo.textContent = "Explicit local JSON, Markdown or HTML transcript import. The original file bytes are kept in the archive. Role labels are required; no URLs are fetched.";
    const genericFile = document.createElement("input");
    genericFile.type = "file";
    genericFile.accept = ".json,.md,.markdown,.html,.htm";
    genericFile.dataset.genericFile = "true";
    genericFile.setAttribute("aria-label", "Choose a transcript file to import");
    const genericButton = document.createElement("button");
    genericButton.type = "button";
    genericButton.className = "quiet-button import-button";
    genericButton.dataset.action = "import-generic";
    genericButton.textContent = "Import selected transcript";
    const genericStatus = document.createElement("p");
    genericStatus.dataset.genericStatus = "true";
    genericStatus.setAttribute("role", "status");
    genericStatus.setAttribute("aria-live", "polite");
    archivePanel.append(document.createElement("hr"), genericTitle, genericInfo, genericFile, genericButton, genericStatus);
    setupMcp = mountLocalMcpSetup(connectionPanel, invoke, safeClipboard);
    mountDocumentRootSetup(documentPanel, invoke);
    const boundaries = document.createElement("ul");
    boundaries.className = "settings-boundaries";
    for (const explanation of [
      "Only an explicitly approved document root may be indexed. Folder labels and paths do not grant additional access.",
      "Staging a folder does not scan it. Indexing begins only after approval and a separate Index folder action.",
      "Lock closes owner-only retrieval and active connections. Revoke permanently removes one folder's permission and invalidates its indexed results.",
      "Capture history is preserved as immutable raw revisions; search indexes can be rebuilt. Hosted access requires separate consent.",
    ]) {
      const item = document.createElement("li");
      item.textContent = explanation;
      boundaries.append(item);
    }
    privacyPanel.append(boundaries);
    setupContent.append(archivePanel, documentPanel, connectionPanel, privacyPanel);
    selectSettingsTab(activeSettingsTab);
  };

  const loadSetup = async () => {
    const version = ++setupVersion;
    setupContent.replaceChildren();
    const note = document.createElement("p");
    note.className = "list-note";
    note.textContent = "Loading setup details…";
    setupContent.append(note);
    try {
      const info = await invoke<SetupInfo>("get_setup");
      if (!alive || version !== setupVersion) return;
      renderSetup(info);
    } catch (error) {
      if (!alive || version !== setupVersion) return;
      setupContent.replaceChildren();
      const message = document.createElement("p");
      message.className = "list-error";
      message.setAttribute("role", "alert");
      message.textContent = `Couldn’t load setup details: ${errorMessage(error)}`;
      const retry = document.createElement("button");
      retry.type = "button";
      retry.className = "text-button";
      retry.dataset.action = "load-setup";
      retry.textContent = "Try again";
      setupContent.append(message, retry);
    }
  };

  const loadConversations = async (reloadSelected = false): Promise<void> => {
    const version = ++listVersion;
    const query = search.value.trim();
    archiveLoadState = "loading";
    if (activeRoute === "overview" && activeWorkspace === "conversations") renderOverview();
    else if (activeRoute === "conversations" && !summaries.length && !selectedId) renderState("Loading your archive", "Looking for saved conversations…", "loading");
    renderListMessage("Searching saved conversations…");
    try {
      const result = await invoke<ConversationSummary[]>("list_conversations", query ? { query } : {});
      if (!alive || version !== listVersion) return;
      const previous = summaries.find((item) => item.id === selectedId);
      const next = result.find((item) => item.id === selectedId);
      const selectedChanged = previous && next && (
        previous.snapshot_count !== next.snapshot_count || previous.captured_at !== next.captured_at
        || previous.message_count !== next.message_count || previous.title !== next.title
        || previous.conversation_date !== next.conversation_date || previous.date_source !== next.date_source
      );
      summaries = result;
      archiveLoadState = "ready";
      if (selectedId && !summaries.some((item) => item.id === selectedId)) {
        selectedId = null;
        selectionVersion++;
        updateToolbarSelection();
      }
      renderConversationRows(query);
      if (activeRoute === "overview" && activeWorkspace === "conversations") renderOverview();
      if (activeRoute === "conversations" && archiveTableVisible) {
        renderArchiveTable();
      } else if (activeRoute === "conversations" && summaries.length && !selectedId) {
        void selectConversation(summaries[0].id);
      } else if (selectedId && (selectedChanged || reloadSelected)) {
        void selectConversation(selectedId);
      } else if (selectedId) {
        updateToolbarSelection(summaries.find((item) => item.id === selectedId));
      }
    } catch (error) {
      if (!alive || version !== listVersion) return;
      archiveLoadState = "error";
      renderListMessage(`Couldn’t load archive: ${errorMessage(error)}`, true);
      if (activeRoute === "overview" && activeWorkspace === "conversations") renderOverview();
      else if (activeRoute === "conversations" && !selectedId) renderState("Archive unavailable", "Check that the local archive service is running, then try again.", "error");
      setActionStatus("Archive could not be refreshed", true);
    }
  };

  // A browser native-host Save writes raw archive evidence independently of
  // this reader. Keep the explicit Refresh action consistent with MCP search:
  // refresh the reader first, then update the rebuildable index once. Avoid
  // indexing on every search keystroke or MCP query.
  let refreshWork: Promise<void> | undefined;
  const refreshArchive = (): Promise<void> => {
    if (refreshWork) return refreshWork;
    refreshWork = (async () => {
      try {
        await loadConversations(true);
        if (!alive) return;
        await invoke("refresh_archive_index");
        if (alive) setActionStatus("Archive and MCP search index refreshed");
      } catch (error) {
        if (alive) setActionStatus(`Archive loaded, but MCP search index could not refresh: ${errorMessage(error)}`, true);
      } finally {
        refreshWork = undefined;
      }
    })();
    return refreshWork;
  };

  const copyAnswer = async (message?: Message) => {
    const current = selectedId ? summaries.find((item) => item.id === selectedId) : undefined;
    if (!current) return;
    const target = message ?? [...(loadedConversation?.capture.messages ?? [])].reverse().find((item) => item.role === "assistant");
    if (!target || target.role !== "assistant") {
      setActionStatus("No assistant answer to copy", true);
      return;
    }
    try {
      const plainText = messagePlainText(renderMessageContent(document, target));
      await safeClipboard.writeText(plainText);
      setActionStatus("Answer copied");
    } catch (error) {
      setActionStatus(`Couldn’t copy answer: ${errorMessage(error)}`, true);
    }
  };

  const exportMarkdown = async () => {
    if (!selectedId) return;
    const id = selectedId;
    const snapshotId = selectedSnapshotId;
    exportButton.disabled = true;
    setActionStatus("Exporting Markdown…");
    try {
      const result = await invoke<ExportResult>("export_markdown", snapshotId ? { id, snapshotId } : { id });
      if (!alive || selectedId !== id || selectedSnapshotId !== snapshotId) return;
      exportedMarkdown = result.markdown;
      const heading = view.querySelector<HTMLElement>(".conversation-heading");
      if (heading) {
        view.querySelector(".export-receipt")?.remove();
        const receipt = document.createElement("div");
        receipt.className = "export-receipt";
        receipt.setAttribute("role", "status");
        const saved = document.createElement("span");
        saved.textContent = result.path
          ? `Markdown saved to ${result.path}`
          : "Markdown is ready in memory, not saved to disk. Copy or explicitly save an unencrypted file.";
        const copy = document.createElement("button");
        copy.type = "button";
        copy.className = "text-button export-copy-button";
        copy.dataset.action = "copy-markdown";
        copy.textContent = "Copy Markdown";
        receipt.append(saved, copy);
        if (!result.path) {
          const save = document.createElement("button");
          save.type = "button";
          save.className = "text-button export-save-button";
          save.dataset.action = "save-markdown";
          save.textContent = "Save Markdown as…";
          receipt.append(save);
        }
        heading.append(receipt);
      }
      setActionStatus(result.path
        ? `Saved Markdown to ${result.path}`
        : "Markdown is in memory only; choose Copy or Save as… to export plaintext.");
    } catch (error) {
      if (alive) setActionStatus(`Couldn’t export Markdown: ${errorMessage(error)}`, true);
    } finally {
      if (alive) exportButton.disabled = !selectedId;
    }
  };

  // Writing decrypted content to disk requires an explicit owner click and
  // browser/OS file picker. Never automatically persist a plaintext export.
  const saveMarkdown = async () => {
    if (!exportedMarkdown) return;
    type Picker = {
      showSaveFilePicker?: (options: {
        suggestedName: string;
        types: Array<{ description: string; accept: Record<string, string[]> }>;
      }) => Promise<{
        createWritable(): Promise<{
          write(content: string): Promise<void>;
          close(): Promise<void>;
        }>;
      }>;
    };
    const host = document.defaultView as (Window & Picker) | null;
    if (!host?.showSaveFilePicker) {
      setActionStatus("Save-as is unavailable in this WebView. Use Copy Markdown instead.", true);
      return;
    }
    const title = selectedId
      ? summaries.find(item => item.id === selectedId)?.title ?? "Mind Place"
      : "Mind Place";
    const suggestedName = (title.replace(/[<>:"/\\|?*\x00-\x1f]/g, "_")
      .replace(/[. ]+$/g, "").slice(0, 80) || "mind-place") + ".md";
    try {
      const handle = await host.showSaveFilePicker({
        suggestedName,
        types: [{ description: "Markdown", accept: { "text/markdown": [".md"] } }],
      });
      const output = await handle.createWritable();
      await output.write(exportedMarkdown);
      await output.close();
      setActionStatus("Markdown saved to the location you selected (unencrypted).");
    } catch (error) {
      if (error instanceof Error && error.name === "AbortError") return;
      setActionStatus(`Couldn’t save Markdown: ${errorMessage(error)}`, true);
    }
  };

  const copyMarkdown = async () => {
    if (!exportedMarkdown) return;
    try {
      await safeClipboard.writeText(exportedMarkdown);
      setActionStatus("Markdown copied");
    } catch (error) {
      setActionStatus(`Couldn’t copy Markdown: ${errorMessage(error)}`, true);
    }
  };

  const registerBrowser = async (browser: "chrome" | "edge" | "brave", button: HTMLButtonElement) => {
    button.disabled = true;
    setActionStatus(`Registering ${titleCase(browser)}…`);
    try {
      const info = await invoke<SetupInfo>("register_extension", { browser });
      if (!alive) return;
      renderSetup(info);
      setActionStatus(`${titleCase(browser)} native-host configuration saved; browser connection has not been tested`);
    } catch (error) {
      if (alive) setActionStatus(`Couldn’t register ${titleCase(browser)}: ${errorMessage(error)}`, true);
    } finally {
      if (alive && button.isConnected) button.disabled = false;
    }
  };

  const importLegacy = async (button: HTMLButtonElement) => {
    button.disabled = true;
    setActionStatus("Importing old archive…");
    const importStatus = setupContent.querySelector<HTMLElement>("[data-setup-status='import']");
    if (importStatus) importStatus.textContent = "Import in progress…";
    try {
      const result = await invoke<LegacyImportResult>("import_legacy");
      if (!alive) return;
      const detail = `${result.imported} imported, ${result.skipped} skipped, ${result.failed} failed.`;
      if (importStatus) {
        importStatus.replaceChildren();
        const summary = document.createElement("p");
        summary.textContent = detail;
        importStatus.append(summary);
        if (result.errors.length) {
          const errors = document.createElement("ul");
          for (const text of result.errors) {
            const row = document.createElement("li");
            row.textContent = text;
            errors.append(row);
          }
          importStatus.append(errors);
        }
      }
      setActionStatus(detail);
      if (result.imported > 0) await loadConversations();
    } catch (error) {
      if (alive) {
        if (importStatus) importStatus.textContent = `Import failed: ${errorMessage(error)}`;
        setActionStatus(`Import failed: ${errorMessage(error)}`, true);
      }
    } finally {
      if (alive && button.isConnected) button.disabled = false;
    }
  };

  const importGeneric = async (button: HTMLButtonElement) => {
    const input = setupContent.querySelector<HTMLInputElement>("[data-generic-file]");
    const status = setupContent.querySelector<HTMLElement>("[data-generic-status]");
    const file = input?.files?.[0];
    if (!file) {
      if (status) status.textContent = "Choose a JSON, Markdown or HTML file first.";
      return;
    }
    const suffix = file.name.toLowerCase().split(".").pop();
    const format = suffix === "json" ? "json" : suffix === "md" || suffix === "markdown"
      ? "markdown" : suffix === "html" || suffix === "htm" ? "html" : null;
    if (!format || !file.size || file.size > 8 * 1024 * 1024) {
      if (status) status.textContent = "Unsupported extension, empty file, or more than 8 MiB.";
      return;
    }
    button.disabled = true;
    if (status) status.textContent = "Importing selected transcript…";
    try {
      // Fatal decoding prevents a lossy UTF-8 replacement; retaining BOM preserves raw bytes.
      const raw = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true })
        .decode(await file.arrayBuffer());
      const result = await invoke<GenericImportReceipt>("import_generic_text", { format, raw });
      if (!alive) return;
      if (result.saved) {
        if (status) status.textContent = result.saved.unchanged
          ? "Already imported. Raw source is retained."
          : "Imported " + result.saved.message_count + " messages; raw source retained.";
        setActionStatus("Transcript imported");
        await loadConversations();
      } else {
        if (status) status.textContent = "Raw source retained, but transcript was not normalized: " + result.errors.join("; ");
        setActionStatus("Import needs a supported explicit transcript structure", true);
      }
    } catch (error) {
      if (!alive) return;
      if (status) status.textContent = "Import failed: " + errorMessage(error);
      setActionStatus("Import failed: " + errorMessage(error), true);
    } finally {
      if (alive && button.isConnected) button.disabled = false;
    }
  };

  const closeSetup = () => {
    ++setupVersion; // Ignore setup responses arriving after the dialog closes.
    setupMcp?.destroy(); setupMcp = undefined;
    activateRoute(modalReturnRoute, modalReturnWorkspace);
    // A wizard that was unmounted by the dialog's destination resumes from its saved step.
    if (modalReturnRoute === "setup-onboarding" && !onboardingWorkspace) openOnboardingWorkspace();
    setupContent.querySelector("[data-mcp-secret]")?.replaceChildren();
    if (typeof dialog.close === "function") {
      try { dialog.close(); } catch { dialog.removeAttribute("open"); }
    } else {
      dialog.removeAttribute("open");
    }
    const returnFocus = modalReturnFocus;
    modalReturnFocus = null;
    if (returnFocus?.isConnected) returnFocus.focus({ preventScroll: true });
    else workspaceNav.querySelector<HTMLButtonElement>(`[data-route="${activeRoute}"]`)?.focus({ preventScroll: true });
  };

  const openSetupDialog = (trigger: HTMLElement, workspace: "connections" | "documents", tab: typeof activeSettingsTab, routeOverride?: UiRouteId) => {
    if (dialog.hasAttribute("open")) return;
    modalReturnWorkspace = activeWorkspace;
    modalReturnRoute = activeRoute;
    modalReturnFocus = trigger;
    activeSettingsTab = tab;
    // With an explicit route the final state is identical; skipping the intermediate
    // workspace activation keeps an open Setup & Onboarding wizard mounted behind the dialog.
    if (routeOverride) activateRoute(routeOverride, workspace);
    else activateWorkspace(workspace);
    if (typeof dialog.showModal === "function") {
      try { dialog.showModal(); } catch { dialog.setAttribute("open", ""); }
    } else dialog.setAttribute("open", "");
    const pendingSetup = loadSetup();
    const focusVersion = setupVersion;
    void pendingSetup.then(() => {
      if (!alive || !dialog.hasAttribute("open") || focusVersion !== setupVersion) return;
      if (trigger.dataset.action === "open-import-setup") {
        const fileInput = setupContent.querySelector<HTMLInputElement>("[data-generic-file]");
        fileInput?.focus(); fileInput?.scrollIntoView?.({ block: "nearest" });
      }
      else if (tab === "documents") setupContent.querySelector<HTMLInputElement>(".document-search-form input")?.focus();
      else get<HTMLButtonElement>("[data-action='close-setup']").focus();
    });
  };

  const returnToConversations = () => {
    if (summaries.length) void selectConversation(summaries[0].id);
    else {
      activateWorkspace("conversations");
      renderState("Archive is empty", "No saved conversations are available.");
      focusViewHeading();
    }
    workspaceNav.querySelector<HTMLButtonElement>('[data-route="conversations"]')?.focus({ preventScroll: true });
  };

  const openMemoryInspector = (workspace: "memories" | "projects" | "people" | "topics" = "memories") => {
    providersWorkspace?.destroy(); providersWorkspace = undefined;
    hiveWorkspace?.destroy(); hiveWorkspace = undefined;
    libraryWorkspace?.destroy(); libraryWorkspace = undefined;
    healthWorkspace?.destroy();
    healthWorkspace = undefined;
    activateWorkspace(workspace);
    memoryInspector?.destroy();
    ++selectionVersion;
    selectedId = null;
    loadedConversation = null;
    selectedSnapshotId = null;
    exportedMarkdown = "";
    updateToolbarSelection();
    setActionStatus("");
    renderConversationRows(search.value.trim());
    toolbarTitle.textContent = workspace === "memories" ? "Memory inspector" : workspace[0].toUpperCase() + workspace.slice(1) + " investigation";
    memoryInspector = mountMemoryInspector({
      initialLens: workspace === "memories" ? undefined : workspace,
      view, invoke, copyText: (value) => safeClipboard.writeText(value),
      onClose: () => {
        memoryInspector?.destroy();
        memoryInspector = undefined;
        returnToConversations();
      },
    });
    focusViewHeading();
  };

  const openHealthWorkspace = () => {
    providersWorkspace?.destroy(); providersWorkspace = undefined;
    hiveWorkspace?.destroy(); hiveWorkspace = undefined;
    libraryWorkspace?.destroy(); libraryWorkspace = undefined;
    memoryInspector?.destroy();
    memoryInspector = undefined;
    healthWorkspace?.destroy();
    healthWorkspace = undefined;
    ++selectionVersion;
    selectedId = null;
    loadedConversation = null;
    loadedSnapshots = [];
    selectedSnapshotId = null;
    exportedMarkdown = "";
    updateToolbarSelection();
    setActionStatus("");
    renderConversationRows(search.value.trim());
    activateWorkspace("health");
    toolbarTitle.textContent = "Retrieval health";
    healthWorkspace = mountHealthWorkspace(view, invoke, () => {
      healthWorkspace?.destroy();
      healthWorkspace = undefined;
      returnToConversations();
    });
    focusViewHeading();
  };

  const openLibraryWorkspace = () => {
    providersWorkspace?.destroy(); providersWorkspace = undefined;
    hiveWorkspace?.destroy(); hiveWorkspace = undefined;
    memoryInspector?.destroy(); memoryInspector = undefined;
    healthWorkspace?.destroy(); healthWorkspace = undefined;
    settingsWorkspace?.destroy(); settingsWorkspace = undefined;
    libraryWorkspace?.destroy(); libraryWorkspace = undefined;
    ++selectionVersion;
    selectedId = null;
    loadedConversation = null;
    loadedSnapshots = [];
    selectedSnapshotId = null;
    exportedMarkdown = "";
    updateToolbarSelection();
    activateWorkspace("library");
    toolbarTitle.textContent = "Library search";
    setActionStatus("");
    renderConversationRows(search.value.trim());
    libraryWorkspace = mountLibraryWorkspace(view, invoke, () => {
      libraryWorkspace?.destroy(); libraryWorkspace = undefined;
      returnToConversations();
    }, safeClipboard);
    focusViewHeading();
  };

  const openFilesVaultWorkspace = () => {
    hiveWorkspace?.destroy(); hiveWorkspace = undefined;
    libraryWorkspace?.destroy(); libraryWorkspace = undefined;
    memoryInspector?.destroy(); memoryInspector = undefined;
    healthWorkspace?.destroy(); healthWorkspace = undefined;
    filesVaultWorkspace?.destroy(); filesVaultWorkspace = undefined;
    ++selectionVersion;
    selectedId = null;
    loadedConversation = null;
    loadedSnapshots = [];
    selectedSnapshotId = null;
    exportedMarkdown = "";
    updateToolbarSelection();
    setActionStatus("");
    renderConversationRows(search.value.trim());
    activateWorkspace("documents");
    toolbarTitle.textContent = "Files & Secure Vault";
    filesVaultWorkspace = mountFilesVaultWorkspace(view, invoke);
    focusViewHeading();
  };

  const openHiveWorkspace = () => {
    const initialKey = summaries.find(x => x.id === selectedId)?.id ?? summaries[0]?.id ?? "";
    libraryWorkspace?.destroy(); libraryWorkspace = undefined;
    memoryInspector?.destroy(); memoryInspector = undefined;
    healthWorkspace?.destroy(); healthWorkspace = undefined;
    providersWorkspace?.destroy(); providersWorkspace = undefined;
    settingsWorkspace?.destroy(); settingsWorkspace = undefined;
    hiveWorkspace?.destroy(); hiveWorkspace = undefined;
    ++selectionVersion;
    selectedId = null;
    loadedConversation = null;
    loadedSnapshots = [];
    selectedSnapshotId = null;
    exportedMarkdown = "";
    updateToolbarSelection();
    setActionStatus("");
    renderConversationRows(search.value.trim());
    activateWorkspace("graph");
    toolbarTitle.textContent = "Hive graph";
    hiveWorkspace = mountHiveWorkspace(view, invoke, () => {
      providersWorkspace?.destroy(); providersWorkspace = undefined;
      hiveWorkspace?.destroy(); hiveWorkspace = undefined;
      returnToConversations();
    }, initialKey, (archiveId) => {
      // Indexed source IDs are not archive IDs. Never navigate by guessed identity.
      const summary = summaries.find(item => item.id === archiveId);
      if (!summary) {
        setActionStatus("This graph identity has no matching saved conversation in the loaded archive.", true);
        return;
      }
      void selectConversation(summary.id);
    });
    focusViewHeading();
  };

  const openOverview = () => {
    providersWorkspace?.destroy(); providersWorkspace = undefined;
    hiveWorkspace?.destroy(); hiveWorkspace = undefined;
    libraryWorkspace?.destroy(); libraryWorkspace = undefined;
    healthWorkspace?.destroy(); healthWorkspace = undefined;
    memoryInspector?.destroy(); memoryInspector = undefined;
    settingsWorkspace?.destroy(); settingsWorkspace = undefined;
    ++selectionVersion;
    selectedId = null;
    loadedConversation = null;
    loadedSnapshots = [];
    selectedSnapshotId = null;
    exportedMarkdown = "";
    const hadSearchFilter = Boolean(search.value.trim());
    search.value = "";
    activateRoute("overview", "conversations");
    setActionStatus("");
    if (hadSearchFilter) void loadConversations();
    else {
      renderConversationRows("");
      renderOverview();
    }
    focusViewHeading();
  };

  const openConversations = () => {
    providersWorkspace?.destroy(); providersWorkspace = undefined;
    hiveWorkspace?.destroy(); hiveWorkspace = undefined;
    libraryWorkspace?.destroy(); libraryWorkspace = undefined;
    healthWorkspace?.destroy(); healthWorkspace = undefined;
    memoryInspector?.destroy(); memoryInspector = undefined;
    archiveTableVisible = true;
    ++selectionVersion;
    selectedId = null;
    loadedConversation = null;
    selectedSnapshotId = null;
    exportedMarkdown = "";
    settingsWorkspace?.destroy(); settingsWorkspace = undefined;
    activateRoute("conversations", "conversations");
    const hadLegacyQuery = Boolean(search.value.trim());
    search.value = "";
    if (hadLegacyQuery) {
      renderState("Loading your archive", "Clearing the previous sidebar search…", "loading");
      void loadConversations();
    } else renderArchiveTable();
    focusViewHeading();
  };

  function renderArchiveTable() {
    view.replaceChildren(createConversationArchive(document, summaries, archiveFilters));
    setViewBusy(false); updateToolbarSelection();
  }

  function openProvidersWorkspace() {
    providersWorkspace?.destroy(); providersWorkspace = undefined;
    hiveWorkspace?.destroy(); hiveWorkspace = undefined;
    memoryInspector?.destroy(); memoryInspector = undefined;
    libraryWorkspace?.destroy(); libraryWorkspace = undefined;
    healthWorkspace?.destroy(); healthWorkspace = undefined;
    ++selectionVersion;
    selectedId = null; loadedConversation = null; loadedSnapshots = []; selectedSnapshotId = null; exportedMarkdown = "";
    activateRoute("ai-providers-mcp", "connections"); updateToolbarSelection(); setActionStatus("");
    providersWorkspace = mountProvidersWorkspace(view, invoke, safeClipboard);
    toolbarTitle.textContent = "AI Providers & MCP"; focusViewHeading();
  }
  const openOnboardingWorkspace = () => {
    hiveWorkspace?.destroy(); hiveWorkspace = undefined;
    libraryWorkspace?.destroy(); libraryWorkspace = undefined;
    healthWorkspace?.destroy(); healthWorkspace = undefined;
    memoryInspector?.destroy(); memoryInspector = undefined;
    onboardingWorkspace?.destroy(); onboardingWorkspace = undefined;
    ++selectionVersion;
    selectedId = null;
    loadedConversation = null;
    loadedSnapshots = [];
    selectedSnapshotId = null;
    exportedMarkdown = "";
    updateToolbarSelection();
    setActionStatus("");
    renderConversationRows(search.value.trim());
    activateRoute("setup-onboarding", "connections");
    toolbarTitle.textContent = "Setup & Onboarding";
    onboardingWorkspace = mountOnboardingWorkspace({
      view, invoke, store: progressStore,
      hooks: {
        openRoute: (route, trigger) => navigateToRoute(route, trigger),
        openSetupDetails: (trigger) => openSetupDialog(trigger, "connections", "archive", "setup-onboarding"),
        openConversation: (id, snapshotId) => {
          void selectConversation(id).then(() => {
            if (alive && selectedId === id && snapshotId) return loadSnapshot(snapshotId);
          });
        },
        onFinish: () => openOverview(),
      },
    });
    focusViewHeading();
  };

  const openSettingsOverview = () => {
    ++selectionVersion;
    selectedId = null; loadedConversation = null; loadedSnapshots = []; selectedSnapshotId = null; exportedMarkdown = "";
    providersWorkspace?.destroy(); providersWorkspace = undefined;
    hiveWorkspace?.destroy(); hiveWorkspace = undefined;
    libraryWorkspace?.destroy(); libraryWorkspace = undefined;
    healthWorkspace?.destroy(); healthWorkspace = undefined;
    memoryInspector?.destroy(); memoryInspector = undefined;
    settingsWorkspace?.destroy(); settingsWorkspace = undefined;
    activateWorkspace("settings");
    toolbarTitle.textContent = "Settings";
    setActionStatus("");
    settingsWorkspace = mountSettingsWorkspace(view, invoke, {
      onLock: () => onLock?.(),
      onNavigate: (route) => navigateToRoute(route, view),
      onOpenSetupDialog: (tab) => openSetupDialog(view, "connections", tab, "settings"),
      onClose: returnToConversations,
    });
    setViewBusy(false);
    updateToolbarSelection();
    focusViewHeading();
  };

  function navigateToRoute(route: UiRouteId, trigger: HTMLElement): void {
    switch (route) {
      case "overview": openOverview(); break;
      case "conversations": openConversations(); break;
      case "curated-memory": openMemoryInspector(); break;
      case "hive-mind-graph": openHiveWorkspace(); break;
      case "files-secure-vault": openFilesVaultWorkspace(); break;
      case "ai-providers-mcp": openProvidersWorkspace(); break;
      case "setup-onboarding": openOnboardingWorkspace(); break;
      case "settings": openSettingsOverview(); break;
    }
  }


  function executeShellCommand(command: UiCommandRegistration, trigger: HTMLElement): void {
    if (command.route) { navigateToRoute(command.route, trigger); return; }
    if (command.action === "open-library-workspace") { openLibraryWorkspace(); return; }
    if (command.action === "open-entity-workspace" && (command.workspace === "projects" || command.workspace === "people" || command.workspace === "topics")) {
      openMemoryInspector(command.workspace);
      return;
    }
    if (command.action === "open-health-inspector") { openHealthWorkspace(); return; }
    root.dispatchEvent(new document.defaultView!.CustomEvent("mp:ui-command", { detail: command, bubbles: true }));
  }

  const clickHandler = (event: Event) => {
    if (!alive) return;
    const target = event.target;
    if (!(target instanceof document.defaultView!.Element)) return;
    const actionElement = target.closest<HTMLElement>("[data-action]");
    if (actionElement) {
      const action = actionElement.dataset.action;
      if (action === "navigate-route") {
        const route = actionElement.dataset.route;
        if (route && isUiRouteId(route)) navigateToRoute(route, actionElement);
      }
      else if (action === "lock") onLock?.();
      else if (action === "archive-table") openConversations();
      else if (action === "copy-transcript" && loadedConversation) {
        const text = loadedConversation.capture.messages.map(message => `${titleCase(message.role)}\n${message.text}`).join("\n\n");
        void safeClipboard.writeText(text).then(() => { if (alive) setActionStatus("Transcript copied"); })
          .catch(() => { if (alive) setActionStatus("Clipboard unavailable", true); });
      }
      else if ((action === "transcript-next" || action === "transcript-previous") && loadedConversation) {
        transcriptPage = Math.max(0, Math.min(Math.ceil(loadedConversation.capture.messages.length / 40) - 1,
          transcriptPage + (action === "transcript-next" ? 1 : -1)));
        renderConversation(loadedConversation, selectionVersion, loadedSnapshots, selectedSnapshotId);
        focusViewHeading();
      }
      else if (action === "refresh") void refreshArchive();
      else if (action === "workspace-conversations") {
        if (summaries.length) void selectConversation(selectedId ?? summaries[0].id);
        else { hiveWorkspace?.destroy(); hiveWorkspace = undefined; libraryWorkspace?.destroy(); libraryWorkspace = undefined; memoryInspector?.destroy(); memoryInspector = undefined; healthWorkspace?.destroy(); healthWorkspace = undefined; activateWorkspace("conversations"); renderState("Archive is empty", "No saved conversations are available."); focusViewHeading(); }
      }
      else if (action === "open-memory-inspector") openMemoryInspector();
      else if (action === "open-entity-workspace") {
        const workspace = actionElement.dataset.workspace;
        if (workspace === "projects" || workspace === "people" || workspace === "topics") openMemoryInspector(workspace);
      }
      else if (action === "open-library-workspace") openLibraryWorkspace();
      else if (action === "open-hive-explorer") openHiveWorkspace();
      else if (action === "open-health-inspector") openHealthWorkspace();
      else if (action === "retry-selection" && selectedId) void selectConversation(selectedId);
      else if (action === "open-settings-section") {
        const tab = actionElement.dataset.tab;
        if (tab === "privacy" || tab === "connections") openSetupDialog(actionElement, "connections", tab, "settings");
      }
      else if (action === "open-document-search") openFilesVaultWorkspace();
      else if (action === "open-setup") {
        // The sidebar's Setup & Onboarding entry opens the wizard; the wizard's own
        // "full capture settings" control (no data-route) opens the detailed dialog.
        if (actionElement.dataset.route === "setup-onboarding") openOnboardingWorkspace();
        else openSetupDialog(actionElement, "connections", "archive", "setup-onboarding");
      }
      else if (action === "open-import-setup") openSetupDialog(actionElement, "connections", "archive", "setup-onboarding");
      else if (action === "close-setup") closeSetup();
      else if (action === "load-setup") void loadSetup();
      else if (action === "export-markdown") void exportMarkdown();
      else if (action === "copy-markdown") void copyMarkdown();
      else if (action === "save-markdown") void saveMarkdown();
      else if (action === "copy-answer") void copyAnswer();
      else if (action === "copy-message") {
        const messageId = actionElement.dataset.messageId;
        const message = loadedConversation?.capture.messages.find((item) => item.id === messageId);
        void copyAnswer(message);
      } else if (action === "register-chrome" || action === "register-edge" || action === "register-brave") {
        void registerBrowser(action.slice("register-".length) as "chrome" | "edge" | "brave", actionElement as HTMLButtonElement);
      } else if (action === "import-legacy") void importLegacy(actionElement as HTMLButtonElement);
      else if (action === "import-generic") void importGeneric(actionElement as HTMLButtonElement);
      return;
    }
    const tabButton = target.closest<HTMLButtonElement>("[data-settings-tab]");
    if (tabButton?.dataset.settingsTab) {
      selectSettingsTab(tabButton.dataset.settingsTab as typeof activeSettingsTab, true);
      return;
    }
    const row = target.closest<HTMLElement>("[data-conversation-id]");
    if (row?.dataset.conversationId) void selectConversation(row.dataset.conversationId);
  };

  const searchHandler = () => { list.scrollTop = 0; void loadConversations(); };
  const listScrollHandler = () => {
    if (alive && summaries.length > VIRTUAL_LIST_THRESHOLD) renderConversationRows(search.value.trim());
  };
  const listKeyHandler = (event: KeyboardEvent) => {
    if (!alive || !["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    const row = (event.target as Element).closest?.<HTMLButtonElement>("[data-row-index]");
    if (!row || !summaries.length) return;
    const index = Number(row.dataset.rowIndex);
    if (!Number.isSafeInteger(index)) return;
    const next = event.key === "Home" ? 0 : event.key === "End" ? summaries.length - 1
      : Math.max(0, Math.min(summaries.length - 1, index + (event.key === "ArrowDown" ? 1 : -1)));
    event.preventDefault();
    if (next === index) return;
    if (summaries.length > VIRTUAL_LIST_THRESHOLD) {
      const top = next * CONVERSATION_ROW_HEIGHT;
      const bottom = top + CONVERSATION_ROW_HEIGHT;
      if (top < list.scrollTop) list.scrollTop = top;
      else if (bottom > list.scrollTop + list.clientHeight) list.scrollTop = bottom - Math.max(list.clientHeight, CONVERSATION_ROW_HEIGHT);
    }
    renderConversationRows(search.value.trim());
    list.querySelector<HTMLButtonElement>(`[data-row-index="${next}"]`)?.focus({ preventScroll: true });
  };
  const changeHandler = (event: Event) => {
    const target = event.target;
    if (target instanceof document.defaultView!.HTMLSelectElement && target.dataset.action === "select-snapshot") {
      void loadSnapshot(target.value);
    }
  };
  const keyHandler = (event: KeyboardEvent) => {
    if (event.key === "Escape" && dialog.hasAttribute("open")) {
      event.preventDefault();
      closeSetup();
      return;
    }
    const selectedTab = document.activeElement?.closest<HTMLElement>("[data-settings-tab]");
    if (dialog.hasAttribute("open") && selectedTab && ["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
      event.preventDefault();
      const tabs = [...root.querySelectorAll<HTMLButtonElement>("[data-settings-tab]")];
      const index = tabs.indexOf(selectedTab as HTMLButtonElement);
      const next = event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1
        : (index + (event.key === "ArrowDown" ? 1 : -1) + tabs.length) % tabs.length;
      selectSettingsTab(tabs[next].dataset.settingsTab as typeof activeSettingsTab, true);
      return;
    }
    if (dialog.hasAttribute("open") || event.defaultPrevented) return;
    if (event.key === "/" && !event.ctrlKey && !event.metaKey && !event.altKey) {
      const active = document.activeElement;
      if (active instanceof document.defaultView!.HTMLElement && (active.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(active.tagName))) return;
      event.preventDefault();
      const visibleSearch = view.querySelector<HTMLInputElement>('input[type="search"]');
      if (visibleSearch) visibleSearch.focus();
      else if (activeRoute === "conversations") search.focus();
    }
  };

  const modalCancelHandler = (event: Event) => { event.preventDefault(); if (alive) closeSetup(); };
  dialog.addEventListener("cancel", modalCancelHandler);
  root.addEventListener("click", clickHandler);
  root.addEventListener("change", changeHandler);
  search.addEventListener("input", searchHandler);
  list.addEventListener("scroll", listScrollHandler);
  list.addEventListener("keydown", listKeyHandler);
  root.addEventListener("keydown", keyHandler);

  const ready = loadConversations();
  // First run of a newly created secret opens the wizard once; a finished wizard is never forced again.
  if (openOnboarding && !hasFinishedOnboarding(progressStore)) {
    void ready.then(() => { if (alive) openOnboardingWorkspace(); }).catch(() => {});
  }
  return {
    ready,
    refresh: refreshArchive,
    destroy: () => {
      setupMcp?.destroy(); setupMcp = undefined;
      providersWorkspace?.destroy(); providersWorkspace = undefined;
      filesVaultWorkspace?.destroy(); filesVaultWorkspace = undefined;
      onboardingWorkspace?.destroy(); onboardingWorkspace = undefined;
      hiveWorkspace?.destroy(); hiveWorkspace = undefined;
      libraryWorkspace?.destroy(); libraryWorkspace = undefined;
      healthWorkspace?.destroy();
      healthWorkspace = undefined;
      settingsWorkspace?.destroy();
      settingsWorkspace = undefined;
      memoryInspector?.destroy();
      memoryInspector = undefined;
      alive = false;
      selectionVersion++;
      listVersion++;
      setupVersion++;
      summaries = [];
      loadedConversation = null;
      loadedSnapshots = [];
      selectedId = null;
      selectedSnapshotId = null;
      exportedMarkdown = "";
      for (const input of root.querySelectorAll<HTMLInputElement | HTMLTextAreaElement>("input, textarea")) input.value = "";
      setupContent.replaceChildren();
      if (dialog.open) {
        try { dialog.close(); } catch { dialog.removeAttribute("open"); }
      }
      disposeWorkspaceKeys();
      uiShell.destroy();
      dialog.removeEventListener("cancel", modalCancelHandler);
      root.removeEventListener("click", clickHandler);
      root.removeEventListener("change", changeHandler);
      search.removeEventListener("input", searchHandler);
      list.removeEventListener("scroll", listScrollHandler);
      list.removeEventListener("keydown", listKeyHandler);
      root.removeEventListener("keydown", keyHandler);
      root.replaceChildren();
    },
  };
}

function makeSourceLink(document: Document, sourceUrl: string): HTMLElement {
  let parsed: URL | undefined;
  try { parsed = new URL(sourceUrl); } catch { /* Invalid archive source remains plain text. */ }
  if (parsed && ["http:", "https:"].includes(parsed.protocol)) {
    const link = document.createElement("a");
    link.className = "source-link";
    link.href = parsed.href;
    link.target = "_blank";
    link.rel = "noopener noreferrer";
    link.textContent = parsed.hostname;
    link.setAttribute("aria-label", `Source: ${parsed.href}`);
    return link;
  }
  const text = document.createElement("span");
  text.textContent = "source unavailable";
  return text;
}

function isSafeImageDataUrl(dataUrl: string, mimeType: string): boolean {
  if (dataUrl.length > 32 * 1024 * 1024) return false;
  const prefix = `data:${mimeType};base64,`;
  if (!dataUrl.startsWith(prefix)) return false;
  return /^[A-Za-z0-9+/]*={0,2}$/.test(dataUrl.slice(prefix.length));
}

function messagePlainText(content: HTMLElement): string {
  const blockTags = new Set(["BLOCKQUOTE", "DIV", "H1", "H2", "H3", "H4", "H5", "H6", "LI", "P", "PRE", "TR"]);
  const collect = (node: Node): string => {
    if (node.nodeType === node.TEXT_NODE) return node.textContent ?? "";
    if (node.nodeType !== node.ELEMENT_NODE) return "";
    const element = node as HTMLElement;
    if (element.tagName === "BR") return "\n";
    const text = [...element.childNodes].map(collect).join("");
    return blockTags.has(element.tagName) ? `${text.trimEnd()}\n` : text;
  };
  return collect(content)
    .replace(/\r\n?/g, "\n")
    .replace(/[\t ]+\n/g, "\n")
    .replace(/\n{3,}/g, "\n\n")
    .trimEnd();
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function titleCase(value: string): string {
  return value.length ? `${value[0].toUpperCase()}${value.slice(1)}` : value;
}

function formatDate(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "Date unavailable";
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(date);
}

function formatTime(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? "" : new Intl.DateTimeFormat(undefined, { timeStyle: "short" }).format(date);
}

function formatRelativeDate(value?: string): string {
  if (!value) return "Date unavailable";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "Date unavailable";
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium" }).format(date);
}

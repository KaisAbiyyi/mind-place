import createDOMPurify from "dompurify";
import { mountLocalMcpSetup } from "./mcp-setup.js";
import { mountDocumentRootSetup } from "./document-roots.js";
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
  Message,
  SetupInfo,
} from "./contracts.js";

export type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

export interface ClipboardWriter {
  writeText(value: string): Promise<void>;
}

export interface ReaderOptions {
  root: HTMLElement;
  invoke: Invoke;
  clipboard?: ClipboardWriter;
  onLock?: () => void;
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

const STATIC_APP_SHELL = `
  <a class="skip-link" href="#reader-view">Skip to conversation</a>
  <div class="app-shell">
    <aside class="library" aria-label="Conversation archive">
      <div class="brand-lockup">
        <span class="brand-mark" aria-hidden="true">m</span>
        <span class="brand-name">Mind Place</span>
      </div>
      <div class="library-heading-row">
        <h1>Archive</h1>
        <button class="quiet-button refresh-button" type="button" data-action="refresh" aria-label="Refresh archive">Refresh</button>
      </div>
      <label class="search-label" for="archive-search">Search conversations</label>
      <div class="search-field">
        <svg aria-hidden="true" viewBox="0 0 20 20"><circle cx="8.8" cy="8.8" r="5.6"></circle><path d="m13 13 4 4"></path></svg>
        <input id="archive-search" type="search" autocomplete="off" placeholder="Search titles and messages" />
        <kbd aria-hidden="true">/</kbd>
      </div>
      <div class="list-heading">Saved conversations</div>
      <div id="conversation-list" class="conversation-list" role="list" aria-label="Saved conversations" aria-live="polite">
        <p class="list-note">Loading archive…</p>
      </div>
      <div class="library-footer">
        <div class="offline-note"><span class="offline-dot" aria-hidden="true"></span><span>Stored on this device</span></div>
        <button class="setup-link" type="button" data-action="open-setup">
          <svg aria-hidden="true" viewBox="0 0 20 20"><path d="M10 2.5v2M10 15.5v2M2.5 10h2m11 0h2m-12.28-5.22 1.42 1.42m7.72 7.72 1.42 1.42m0-10.56-1.42 1.42m-7.72 7.72-1.42 1.42"></path><circle cx="10" cy="10" r="4"></circle></svg>
          <span>Manual capture setup</span>
        </button>
      </div>
    </aside>
    <main class="reader-main">
      <header class="reader-toolbar">
        <div class="toolbar-context"><span class="context-mark" aria-hidden="true"></span><span>Conversation reader</span></div>
        <div class="toolbar-actions">
          <button class="quiet-button" type="button" data-action="lock">Lock</button>
          <span id="action-status" class="action-status" role="status" aria-live="polite"></span>
          <button class="quiet-button" type="button" data-action="copy-answer" disabled>Copy answer</button>
          <button class="primary-button" type="button" data-action="export-markdown" disabled>
            <svg aria-hidden="true" viewBox="0 0 20 20"><path d="M10 2.75v9.5m0 0 3.25-3.25M10 12.25 6.75 9M3.5 13.75v2.5c0 .55.45 1 1 1h11c.55 0 1-.45 1-1v-2.5"></path></svg>
            <span>Export Markdown</span>
          </button>
        </div>
      </header>
      <section id="reader-view" class="reader-view" tabindex="-1" aria-live="polite" aria-busy="true">
        <div class="state-panel loading-panel"><span class="loading-mark" aria-hidden="true"></span><p>Opening your archive…</p></div>
      </section>
    </main>
  </div>
  <dialog id="setup-dialog" class="setup-dialog" aria-labelledby="setup-title">
    <div class="dialog-header">
      <div><p class="eyebrow">Local connection</p><h2 id="setup-title">Manual capture setup</h2></div>
      <button class="icon-button" type="button" data-action="close-setup" aria-label="Close setup">×</button>
    </div>
    <div id="setup-content" class="setup-content">
      <p class="list-note">Loading setup details…</p>
    </div>
    <div class="dialog-footer"><button class="quiet-button" type="button" data-action="lock">Lock</button><button class="quiet-button" type="button" data-action="close-setup">Done</button></div>
  </dialog>
`;

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

export function mountReader({ root, invoke: nativeInvoke, clipboard, onLock }: ReaderOptions): ReaderHandle {
  const document = root.ownerDocument;
  const clipboardWriter = clipboard ?? {
    writeText: (value: string) => document.defaultView?.navigator.clipboard?.writeText(value)
      ?? Promise.reject(new Error("Clipboard access is unavailable in this app window.")),
  };

  root.innerHTML = STATIC_APP_SHELL;
  let summaries: ConversationSummary[] = [];
  let selectedId: string | null = null;
  let selectionVersion = 0;
  let listVersion = 0;
  let setupVersion = 0;
  let loadedConversation: Conversation | null = null;
  let loadedSnapshots: CaptureSnapshot[] = [];
  let selectedSnapshotId: string | null = null;
  let exportedMarkdown = "";
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
  const view = get<HTMLElement>("#reader-view");
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
    toolbarTitle.textContent = summary ? "Conversation reader" : "Your archive";
    copyButton.disabled = !summary;
    exportButton.disabled = !summary;
  };

  const renderConversationRows = (query: string) => {
    list.replaceChildren();
    for (const item of summaries) {
      const row = document.createElement("button");
      row.type = "button";
      row.className = "conversation-row";
      row.dataset.conversationId = item.id;
      row.setAttribute("role", "listitem");
      row.setAttribute("aria-pressed", String(item.id === selectedId));

      const title = document.createElement("span");
      title.className = "conversation-title";
      title.textContent = item.title || "Untitled conversation";
      const meta = document.createElement("span");
      meta.className = "conversation-meta";
      meta.textContent = `${formatRelativeDate(item.conversation_date)} · ${item.message_count} messages`;
      row.append(title, meta);
      if (item.completeness === "partial") {
        const marker = document.createElement("span");
        marker.className = "partial-marker";
        marker.textContent = "Partial capture";
        row.append(marker);
      }
      list.append(row);
    }

    if (summaries.length === 0) {
      renderListMessage(query ? "No conversations match this search." : "No saved conversations yet.");
      if (!selectedId) {
        renderState(
          query ? "No matches" : "Your archive is empty",
          query ? "Try another title or phrase." : "Save a conversation with the browser extension. Your saved chats stay on this device.",
        );
      }
    }
  };

  const selectConversation = async (id: string) => {
    selectedId = id;
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
    const metadataParts: Node[] = [originalDate];
    if (capture.conversation_created_at && summary.date_source !== "provider-created") {
      const created = document.createElement("span");
      created.textContent = `Created ${formatDate(capture.conversation_created_at)}`;
      metadataParts.push(document.createTextNode(" · "), created);
    }
    const date = document.createElement("span");
    date.textContent = `Captured ${formatDate(capture.captured_at)}`;
    const messageCount = document.createElement("span");
    messageCount.textContent = `${capture.messages.length} messages in this snapshot`;
    const captureOrigin = document.createElement("span");
    captureOrigin.textContent = capture.capture_method === "legacy-import" ? "Imported legacy archive"
      : capture.capture_method === "automatic-dom" ? "Automatic browser save · loaded messages only" : "Manual browser save";
    const origin = makeSourceLink(document, capture.source_url || summary.source_url);
    metadataParts.push(
      document.createTextNode(" · "), date,
      document.createTextNode(" · "), messageCount,
      document.createTextNode(" · "), captureOrigin,
      document.createTextNode(" · "), origin,
    );
    metadata.append(...metadataParts);
    top.append(eyebrow, heading, metadata);
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
    for (const message of capture.messages) {
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
        copy.setAttribute("aria-label", `Copy answer ${messageList.querySelectorAll(".message-assistant").length + 1}`);
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
    view.replaceChildren(top, transcript);
    setViewBusy(false);
    updateToolbarSelection(summary);
  }

  const renderSetup = (info: SetupInfo) => {
    setupContent.replaceChildren();
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
      ? `Registered in ${info.registered_browsers.map(titleCase).join(" and ")}.`
      : "The browser extension has not been registered yet.";

    const register = document.createElement("div");
    register.className = "register-actions";
    for (const browser of ["chrome", "edge"] as const) {
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

    setupContent.append(connection, pathList, registered, register, divider, legacyTitle, legacyText, legacyButton, importStatus);
    mountLocalMcpSetup(setupContent, invoke, safeClipboard);
    mountDocumentRootSetup(setupContent, invoke);
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
    if (!summaries.length && !selectedId) renderState("Loading your archive", "Looking for saved conversations…", "loading");
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
      if (selectedId && !summaries.some((item) => item.id === selectedId)) {
        selectedId = null;
        selectionVersion++;
        updateToolbarSelection();
      }
      renderConversationRows(query);
      if (summaries.length && !selectedId) {
        void selectConversation(summaries[0].id);
      } else if (selectedId && (selectedChanged || reloadSelected)) {
        void selectConversation(selectedId);
      } else if (selectedId) {
        updateToolbarSelection(summaries.find((item) => item.id === selectedId));
      }
    } catch (error) {
      if (!alive || version !== listVersion) return;
      renderListMessage(`Couldn’t load archive: ${errorMessage(error)}`, true);
      if (!selectedId) renderState("Archive unavailable", "Check that the local archive service is running, then try again.", "error");
      setActionStatus("Archive could not be refreshed", true);
    }
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
        saved.textContent = `Markdown saved to ${result.path}`;
        const copy = document.createElement("button");
        copy.type = "button";
        copy.className = "text-button export-copy-button";
        copy.dataset.action = "copy-markdown";
        copy.textContent = "Copy Markdown";
        receipt.append(saved, copy);
        heading.append(receipt);
      }
      setActionStatus(`Saved Markdown to ${result.path}`);
    } catch (error) {
      if (alive) setActionStatus(`Couldn’t export Markdown: ${errorMessage(error)}`, true);
    } finally {
      if (alive) exportButton.disabled = !selectedId;
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

  const registerBrowser = async (browser: "chrome" | "edge", button: HTMLButtonElement) => {
    button.disabled = true;
    setActionStatus(`Registering ${titleCase(browser)}…`);
    try {
      const info = await invoke<SetupInfo>("register_extension", { browser });
      if (!alive) return;
      renderSetup(info);
      setActionStatus(`${titleCase(browser)} extension registered`);
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

  const closeSetup = () => {
    setupContent.querySelector("[data-mcp-secret]")?.replaceChildren();
    if (typeof dialog.close === "function") {
      try { dialog.close(); } catch { dialog.removeAttribute("open"); }
    } else {
      dialog.removeAttribute("open");
    }
    get<HTMLButtonElement>("[data-action='open-setup']").focus();
  };

  const clickHandler = (event: Event) => {
    if (!alive) return;
    const target = event.target;
    if (!(target instanceof document.defaultView!.Element)) return;
    const actionElement = target.closest<HTMLElement>("[data-action]");
    if (actionElement) {
      const action = actionElement.dataset.action;
      if (action === "lock") onLock?.();
      else if (action === "refresh") void loadConversations(true);
      else if (action === "retry-selection" && selectedId) void selectConversation(selectedId);
      else if (action === "open-setup") {
        if (typeof dialog.showModal === "function") {
          try { dialog.showModal(); } catch { dialog.setAttribute("open", ""); }
        } else dialog.setAttribute("open", "");
        void loadSetup();
      } else if (action === "close-setup") closeSetup();
      else if (action === "load-setup") void loadSetup();
      else if (action === "export-markdown") void exportMarkdown();
      else if (action === "copy-markdown") void copyMarkdown();
      else if (action === "copy-answer") void copyAnswer();
      else if (action === "copy-message") {
        const messageId = actionElement.dataset.messageId;
        const message = loadedConversation?.capture.messages.find((item) => item.id === messageId);
        void copyAnswer(message);
      } else if (action === "register-chrome" || action === "register-edge") {
        void registerBrowser(action.slice("register-".length) as "chrome" | "edge", actionElement as HTMLButtonElement);
      } else if (action === "import-legacy") void importLegacy(actionElement as HTMLButtonElement);
      return;
    }
    const row = target.closest<HTMLElement>("[data-conversation-id]");
    if (row?.dataset.conversationId) void selectConversation(row.dataset.conversationId);
  };

  const searchHandler = () => { void loadConversations(); };
  const changeHandler = (event: Event) => {
    const target = event.target;
    if (target instanceof document.defaultView!.HTMLSelectElement && target.dataset.action === "select-snapshot") {
      void loadSnapshot(target.value);
    }
  };
  const keyHandler = (event: KeyboardEvent) => {
    if (event.key === "Escape" && dialog.hasAttribute("open")) closeSetup();
    if (event.key === "/" && !event.ctrlKey && !event.metaKey && !event.altKey) {
      const active = document.activeElement;
      if (active instanceof document.defaultView!.HTMLElement && /^(INPUT|TEXTAREA|SELECT)$/.test(active.tagName)) return;
      event.preventDefault();
      search.focus();
    }
  };

  root.addEventListener("click", clickHandler);
  root.addEventListener("change", changeHandler);
  search.addEventListener("input", searchHandler);
  root.addEventListener("keydown", keyHandler);

  const ready = loadConversations();
  return {
    ready,
    refresh: () => loadConversations(true),
    destroy: () => {
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
      root.removeEventListener("click", clickHandler);
      root.removeEventListener("change", changeHandler);
      search.removeEventListener("input", searchHandler);
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

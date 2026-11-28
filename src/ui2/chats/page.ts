import type { ConversationSummary } from "../../contracts.js";
import type { ClipboardWriter, Invoke } from "../../reader.js";
import type { Ui2PageContext, Ui2PageHandle } from "../shell/page.js";
import { createUi2Button } from "../ui-kit/index.js";
import { createArchiveList } from "./archive-list.js";
import { createChatReader, type ChatReaderHandle } from "./chat-reader.js";

export interface ArchivePageDeps extends Ui2PageContext {
  invoke: Invoke;
  clipboard?: ClipboardWriter;
}

/** Where the "set up saving" action goes when the archive is empty. */
const CONNECTIONS_PATH = "/connections";

/**
 * MP-UI2-06: the Archive destination. A list of everything saved or imported,
 * and a reader that looks like a chat app. Metadata sits behind Info.
 *
 * Importing needs a native file picker command that does not exist yet, so the
 * Import button is shown but unavailable, and says why. Nothing here pretends to import.
 */
export function createArchivePage(deps: ArchivePageDeps): Ui2PageHandle {
  const { doc, i18n, invoke, clipboard } = deps;
  const root = doc.createElement("div");
  root.className = "ui2-stack";

  // List view
  const listView = doc.createElement("div");
  listView.className = "ui2-stack";
  const head = doc.createElement("div");
  head.className = "ui2-chats-head";
  const lead = doc.createElement("p");
  lead.className = "ui2-lead";
  const importHint = doc.createElement("p");
  importHint.className = "ui2-chats-note";
  importHint.id = "ui2-archive-import-hint";
  let importButton: HTMLButtonElement | undefined;

  const list = createArchiveList({
    doc,
    i18n,
    onOpen: (summary) => openChat(summary),
    onRetry: () => { void loadList(); },
    ...(deps.navigate ? { onSetup: () => deps.navigate?.(CONNECTIONS_PATH) } : {}),
  });
  listView.append(head, importHint, list.element);

  function paintHead(): void {
    lead.textContent = i18n.t("arch.lead");
    importHint.textContent = i18n.t("arch.importUnavailable");
    importButton = createUi2Button(doc, {
      label: i18n.t("action.import"), variant: "primary", icon: "plus", disabled: true,
    });
    importButton.setAttribute("aria-describedby", importHint.id);
    head.replaceChildren(lead, importButton);
  }
  paintHead();
  root.append(listView);

  let alive = true;
  let listVersion = 0;
  let reader: ChatReaderHandle | undefined;
  let openedId: string | null = null;

  async function loadList(): Promise<void> {
    const version = ++listVersion;
    list.setLoading();
    try {
      const result = await invoke<ConversationSummary[]>("list_conversations", {});
      if (!alive || version !== listVersion) return;
      list.setSummaries(result);
    } catch {
      if (!alive || version !== listVersion) return;
      list.setError();
    }
  }

  function closeChat(): void {
    if (!reader) return;
    const restore = openedId;
    reader.destroy();
    reader = undefined;
    openedId = null;
    listView.hidden = false;
    deps.setDetail?.(null);
    if (restore) list.focusRow(restore);
  }

  function openChat(summary: ConversationSummary): void {
    if (reader) return;
    openedId = summary.id;
    listView.hidden = true;
    reader = createChatReader({
      doc, i18n, invoke, clipboard, summary,
      onBack: closeChat,
      onTitle: (title) => deps.setDetail?.({ title, onBack: closeChat }),
    });
    root.append(reader.element);
    deps.setDetail?.({ title: summary.title || i18n.t("arch.untitled"), onBack: closeChat });
    reader.focusTitle();
  }

  const unsubscribe = i18n.subscribe(() => {
    paintHead();
    list.rebuild();
    reader?.rebuild();
  });

  void loadList();

  return {
    element: root,
    destroy() {
      alive = false;
      listVersion += 1;
      unsubscribe();
      reader?.destroy();
      reader = undefined;
      list.destroy();
      root.replaceChildren();
      root.remove();
    },
  };
}

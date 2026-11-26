import type { ExportResult } from "../../contracts.js";
import type { ClipboardWriter, Invoke } from "../../reader.js";
import type { Ui2I18nController } from "../i18n/index.js";
import { createUi2Aside, createUi2Button, type Ui2AsideHandle } from "../ui-kit/index.js";

export interface ExportTarget {
  id: string;
  snapshotId: string | null;
  title: string;
}

export interface ExportPanelDeps {
  doc: Document;
  i18n: Ui2I18nController;
  invoke: Invoke;
  clipboard?: ClipboardWriter | undefined;
  /** The chat copy being read right now, or null when none is open. */
  target(): ExportTarget | null;
  onClose(): void;
}

export interface ExportPanel {
  readonly aside: Ui2AsideHandle;
  /** Forget any prepared file and clear messages (a different copy is on screen). */
  reset(): void;
  destroy(): void;
}

type SavePicker = {
  showSaveFilePicker?: (options: {
    suggestedName: string;
    types: Array<{ description: string; accept: Record<string, string[]> }>;
  }) => Promise<{ createWritable(): Promise<{ write(content: string): Promise<void>; close(): Promise<void> }> }>;
};

function suggestedFileName(title: string): string {
  const cleaned = title.replace(/[<>:"/\\|?*\u0000-\u001f]/g, "_").replace(/[. ]+$/g, "").slice(0, 80);
  return (cleaned || "mind-place") + ".md";
}

/**
 * Export for one chat copy. Markdown is the only format the app can produce, so
 * there is one button. The classic export command decides whether a file is
 * written (it never is for an encrypted archive: the text stays in memory and the
 * owner chooses Copy or Save as).
 */
export function createExportPanel(deps: ExportPanelDeps): ExportPanel {
  const { doc, i18n, invoke } = deps;
  const win = doc.defaultView;
  const aside = createUi2Aside(doc, {
    title: i18n.t("arch.export.title"),
    closeLabel: i18n.t("action.close"),
    onClose: deps.onClose,
  });
  aside.element.classList.add("ui2-chats-aside");

  const lead = doc.createElement("p");
  lead.className = "ui2-chats-note";
  lead.textContent = i18n.t("arch.export.lead");
  const run = createUi2Button(doc, { label: i18n.t("arch.export.markdown"), variant: "primary", icon: "download" });
  const status = doc.createElement("p");
  status.className = "ui2-chats-note";
  status.setAttribute("role", "status");
  const actions = doc.createElement("div");
  actions.className = "ui2-row-flex";
  aside.body.append(lead, run, status, actions);

  let markdown = "";
  let token = 0;

  function setStatus(text: string): void {
    status.textContent = text;
  }

  function reset(): void {
    token += 1;
    markdown = "";
    run.disabled = false;
    setStatus("");
    actions.replaceChildren();
  }

  async function copyMarkdown(): Promise<void> {
    const writer = deps.clipboard ?? win?.navigator.clipboard;
    try {
      if (!writer || !markdown) throw new Error("MP_UI2_CLIPBOARD_UNAVAILABLE");
      await writer.writeText(markdown);
      setStatus(i18n.t("arch.export.copied"));
    } catch {
      setStatus(i18n.t("arch.copyError"));
    }
  }

  async function saveAs(title: string): Promise<void> {
    const host = win as (Window & SavePicker) | null;
    if (!host?.showSaveFilePicker || !markdown) {
      setStatus(i18n.t("arch.export.saveUnavailable"));
      return;
    }
    try {
      const handle = await host.showSaveFilePicker({
        suggestedName: suggestedFileName(title),
        types: [{ description: i18n.t("arch.export.markdown"), accept: { "text/markdown": [".md"] } }],
      });
      const output = await handle.createWritable();
      await output.write(markdown);
      await output.close();
      setStatus(i18n.t("arch.export.saved"));
    } catch (error) {
      if (error instanceof Error && error.name === "AbortError") return;
      setStatus(i18n.t("arch.export.error"));
    }
  }

  run.addEventListener("click", () => {
    const target = deps.target();
    if (!target) return;
    reset();
    const mine = token;
    run.disabled = true;
    setStatus(i18n.t("arch.export.working"));
    void invoke<ExportResult>("export_markdown", target.snapshotId ? { id: target.id, snapshotId: target.snapshotId } : { id: target.id })
      .then((result) => {
        if (mine !== token) return;
        markdown = result.markdown;
        setStatus(result.path
          ? i18n.t("arch.export.savedFile", { location: result.path })
          : i18n.t("arch.export.ready"));
        actions.append(createUi2Button(doc, {
          label: i18n.t("arch.export.copyMarkdown"),
          icon: "copy",
          onClick: () => { void copyMarkdown(); },
        }));
        if (!result.path) {
          actions.append(createUi2Button(doc, {
            label: i18n.t("arch.export.saveAs"),
            onClick: () => { void saveAs(target.title); },
          }));
        }
        run.disabled = false;
      })
      .catch(() => {
        if (mine !== token) return;
        setStatus(i18n.t("arch.export.error"));
        run.disabled = false;
      });
  });

  return {
    aside,
    reset,
    destroy() {
      token += 1;
      markdown = "";
      aside.element.remove();
    },
  };
}

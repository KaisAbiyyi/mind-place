import type { Capture, CaptureSnapshot, ConversationSummary } from "../../contracts.js";
import type { Ui2I18nController } from "../i18n/index.js";
import { createUi2Button, createUi2Tag, createUi2TechnicalDetails, type Ui2TechnicalEntry } from "../ui-kit/index.js";
import { messagesLabel, providerName, safeHttpHost } from "./format.js";

/** Saved copies listed before "Show more". */
export const COPIES_PAGE = 8;

export interface InfoState {
  /** How many saved copies are listed right now. */
  copiesShown: number;
  /** A copy is being opened: its buttons wait. */
  busy: boolean;
  /** Opening a copy failed: say so, change nothing. */
  copyFailed: boolean;
}

export interface InfoDeps {
  doc: Document;
  i18n: Ui2I18nController;
  summary: ConversationSummary;
  capture: Capture;
  snapshots: readonly CaptureSnapshot[];
  activeSnapshotId: string | null;
  state: InfoState;
  onSelectCopy(snapshotId: string): void;
  onShowMoreCopies(): void;
}

export interface InfoBody {
  readonly element: HTMLElement;
  /** Where "Why?" lands: the Completeness group heading. */
  readonly completenessHeading: HTMLElement;
}

function group(doc: Document, title: string): { section: HTMLElement; heading: HTMLElement } {
  const section = doc.createElement("section");
  section.className = "ui2-chats-group";
  const heading = doc.createElement("h4");
  heading.className = "ui2-chats-group-title";
  heading.tabIndex = -1;
  heading.textContent = title;
  section.append(heading);
  return { section, heading };
}

function row(doc: Document, label: string, value: string | Node): HTMLElement {
  const wrap = doc.createElement("div");
  wrap.className = "ui2-chats-kv";
  const dt = doc.createElement("dt");
  dt.textContent = label;
  const dd = doc.createElement("dd");
  dd.append(value);
  wrap.append(dt, dd);
  return wrap;
}

function howSaved(i18n: Ui2I18nController, capture: Capture): string {
  if (capture.capture_method === "manual-dom") return i18n.t("arch.info.howManual");
  if (capture.capture_method === "automatic-dom") return i18n.t("arch.info.howAuto");
  if (capture.capture_method === "legacy-import") return i18n.t("arch.info.howLegacy");
  return i18n.t("state.unknown");
}

/** Grouped metadata for the Info aside. Identifiers live only inside Technical details. */
export function buildInfoBody(deps: InfoDeps): InfoBody {
  const { doc, i18n, summary, capture, snapshots, activeSnapshotId, state } = deps;
  const root = doc.createElement("div");
  root.className = "ui2-chats-info";

  // Source
  const source = group(doc, i18n.t("arch.info.source"));
  const facts = doc.createElement("dl");
  facts.className = "ui2-chats-facts";
  facts.append(
    row(doc, i18n.t("arch.info.app"), providerName(summary.provider) || i18n.t("state.unknown")),
    row(doc, i18n.t("arch.info.saved"), i18n.formatDate(capture.captured_at, "medium")),
    row(doc, i18n.t("arch.info.how"), howSaved(i18n, capture)),
  );
  const page = safeHttpHost(capture.source_url || summary.source_url);
  if (page) {
    const link = doc.createElement("a");
    link.href = page.href;
    link.target = "_blank";
    link.rel = "noopener noreferrer";
    link.textContent = page.host;
    facts.append(row(doc, i18n.t("arch.info.page"), link));
  }
  source.section.append(facts);
  root.append(source.section);

  // Saved copies
  if (snapshots.length > 0) {
    const copies = group(doc, i18n.t("arch.info.copies"));
    const list = doc.createElement("ul");
    list.className = "ui2-chats-copies";
    snapshots.slice(0, state.copiesShown).forEach((snapshot, index) => {
      const item = doc.createElement("li");
      item.className = "ui2-chats-copy";
      const current = snapshot.snapshot_id === activeSnapshotId;
      if (current) {
        item.tabIndex = -1;
        item.dataset.ui2CopyActive = "true";
      }
      const text = doc.createElement("div");
      text.className = "ui2-chats-copy-text";
      const title = doc.createElement("span");
      title.className = "ui2-chats-copy-title";
      title.textContent = i18n.t(index === 0 ? "arch.info.copyLatest" : "arch.info.copyEarlier");
      const meta = doc.createElement("span");
      meta.className = "ui2-chats-copy-meta";
      meta.textContent = i18n.t("arch.info.copyMeta", {
        date: i18n.formatDate(snapshot.captured_at, "medium"),
        messages: messagesLabel(i18n, snapshot.message_count),
      });
      text.append(title, meta);
      item.append(text);
      if (current) {
        item.append(createUi2Tag(doc, { label: i18n.t("arch.info.copyShowing"), tone: "ok" }));
      } else {
        item.append(createUi2Button(doc, {
          label: i18n.t("arch.info.copyView"),
          size: "sm",
          disabled: state.busy,
          onClick: () => deps.onSelectCopy(snapshot.snapshot_id),
        }));
      }
      list.append(item);
    });
    copies.section.append(list);
    if (snapshots.length > state.copiesShown) {
      copies.section.append(createUi2Button(doc, {
        label: i18n.t("arch.info.copiesMore"),
        size: "sm",
        variant: "ghost",
        onClick: () => deps.onShowMoreCopies(),
      }));
    }
    if (state.copyFailed) {
      const failed = doc.createElement("p");
      failed.className = "ui2-chats-note ui2-chats-note-warn";
      failed.setAttribute("role", "alert");
      failed.textContent = i18n.t("arch.info.copyError");
      copies.section.append(failed);
    }
    const note = doc.createElement("p");
    note.className = "ui2-chats-note";
    note.textContent = i18n.t("arch.info.copiesNote");
    copies.section.append(note);
    root.append(copies.section);
  }

  // Completeness
  const partial = capture.completeness === "partial" || summary.completeness === "partial";
  const complete = group(doc, i18n.t("arch.info.complete"));
  const completeText = doc.createElement("p");
  completeText.className = "ui2-chats-note";
  completeText.textContent = i18n.t(partial ? "arch.info.completePartial" : "arch.info.completeFull");
  complete.section.append(completeText);
  root.append(complete.section);

  // Technical details (collapsed)
  const entries: Ui2TechnicalEntry[] = [{ label: i18n.t("arch.info.chatId"), value: summary.id }];
  if (activeSnapshotId) entries.push({ label: i18n.t("arch.info.copyId"), value: activeSnapshotId });
  if (capture.warnings.length) entries.push({ label: i18n.t("arch.info.notes"), value: capture.warnings.join(" ") });
  root.append(createUi2TechnicalDetails(doc, { summaryLabel: i18n.t("arch.info.tech"), entries }));

  return { element: root, completenessHeading: complete.heading };
}

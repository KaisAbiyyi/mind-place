import type { UiSurfaceStatus } from "./ui-contracts.js";
export type UiButtonVariant = "primary" | "secondary" | "quiet" | "link";
export interface UiButtonOptions {
  readonly variant?: UiButtonVariant;
  readonly type?: "button" | "submit" | "reset";
  readonly disabled?: boolean;
  readonly ariaLabel?: string;
  readonly className?: string;
}
export function createUiButton(document: Document, label: string, options: UiButtonOptions = {}): HTMLButtonElement {
  const variant = options.variant ?? "secondary";
  const button = document.createElement("button");
  button.type = options.type ?? "button";
  button.className = ["ui-button", "ui-button-" + variant, options.className ?? ""].filter(Boolean).join(" ");
  button.dataset.variant = variant;
  button.textContent = label;
  button.disabled = options.disabled ?? false;
  if (options.ariaLabel) button.setAttribute("aria-label", options.ariaLabel);
  return button;
}
export interface UiCardParts { readonly element: HTMLElement; readonly body: HTMLElement; }
export interface UiCardOptions { readonly title?: string; readonly description?: string; readonly className?: string; }
export function createUiCard(document: Document, options: UiCardOptions = {}): UiCardParts {
  const card = document.createElement("section");
  card.className = ["ui-card", options.className ?? ""].filter(Boolean).join(" ");
  if (options.title || options.description) {
    const header = document.createElement("header");
    header.className = "ui-card-header";
    if (options.title) {
      const title = document.createElement("h2");
      title.className = "ui-card-title";
      title.textContent = options.title;
      header.append(title);
    }
    if (options.description) {
      const description = document.createElement("p");
      description.className = "ui-card-description";
      description.textContent = options.description;
      header.append(description);
    }
    card.append(header);
  }
  const body = document.createElement("div");
  body.className = "ui-card-body";
  card.append(body);
  return { element: card, body };
}
export type UiStatusState = "neutral" | "complete" | "partial" | "loading" | "error" | "locked" | "permission-denied" | "disconnected" | "revoked";
export function createUiStatus(document: Document, label: string, state: UiStatusState = "neutral"): HTMLSpanElement {
  const status = document.createElement("span");
  status.className = "ui-status";
  status.dataset.state = state;
  status.setAttribute("role", "status");
  status.setAttribute("aria-live", "polite");
  const mark = document.createElement("span");
  mark.className = "ui-status-mark";
  mark.setAttribute("aria-hidden", "true");
  const text = document.createElement("span");
  text.textContent = label;
  status.append(mark, text);
  return status;
}
export type UiTableCell = string | Node;
export interface UiTableOptions {
  readonly caption: string;
  readonly columns: readonly string[];
  readonly rows: readonly (readonly UiTableCell[])[];
  readonly emptyLabel?: string;
}
export function createUiTable(document: Document, options: UiTableOptions): HTMLDivElement {
  const wrapper = document.createElement("div");
  wrapper.className = "ui-table-scroll";
  const table = document.createElement("table");
  table.className = "ui-table";
  const caption = document.createElement("caption");
  caption.textContent = options.caption;
  table.append(caption);
  const head = document.createElement("thead");
  const headerRow = document.createElement("tr");
  for (const column of options.columns) {
    const cell = document.createElement("th");
    cell.scope = "col";
    cell.textContent = column;
    headerRow.append(cell);
  }
  head.append(headerRow);
  table.append(head);
  const body = document.createElement("tbody");
  if (options.rows.length === 0 && options.emptyLabel) {
    const row = document.createElement("tr");
    const cell = document.createElement("td");
    cell.colSpan = Math.max(1, options.columns.length);
    cell.className = "ui-table-empty";
    cell.textContent = options.emptyLabel;
    row.append(cell);
    body.append(row);
  } else {
    for (const values of options.rows) {
      const row = document.createElement("tr");
      for (const value of values) {
        const cell = document.createElement("td");
        if (typeof value === "string") cell.textContent = value;
        else cell.append(value.cloneNode(true));
        row.append(cell);
      }
      body.append(row);
    }
  }
  table.append(body);
  wrapper.append(table);
  return wrapper;
}
export interface UiDialogOptions { readonly id: string; readonly title: string; readonly description?: string; readonly className?: string; }
export function createUiDialog(document: Document, options: UiDialogOptions): HTMLDialogElement {
  const dialog = document.createElement("dialog");
  dialog.id = options.id;
  dialog.className = ["ui-dialog", options.className ?? ""].filter(Boolean).join(" ");
  const titleId = options.id + "-title";
  dialog.setAttribute("aria-labelledby", titleId);
  const header = document.createElement("header");
  header.className = "ui-dialog-header";
  const title = document.createElement("h2");
  title.id = titleId;
  title.textContent = options.title;
  const close = createUiButton(document, "Close", { variant: "quiet", ariaLabel: "Close " + options.title, className: "ui-dialog-close" });
  close.dataset.action = "close-ui-dialog";
  close.dataset.dialogId = options.id;
  header.append(title, close);
  dialog.append(header);
  if (options.description) {
    const description = document.createElement("p");
    description.className = "ui-dialog-description";
    description.textContent = options.description;
    description.id = options.id + "-description";
    dialog.setAttribute("aria-describedby", description.id);
    dialog.append(description);
  }
  const body = document.createElement("div");
  body.className = "ui-dialog-body";
  dialog.append(body);
  return dialog;
}
export interface UiDrawerOptions extends UiDialogOptions { readonly placement?: "left" | "right"; }
export function createUiDrawer(document: Document, options: UiDrawerOptions): HTMLDialogElement {
  const drawer = createUiDialog(document, options);
  drawer.classList.add("ui-drawer");
  drawer.dataset.placement = options.placement ?? "right";
  return drawer;
}
export interface UiStatePanelOptions { readonly state: UiSurfaceStatus; readonly title: string; readonly detail: string; }
export function createUiStatePanel(document: Document, options: UiStatePanelOptions): HTMLElement {
  const panel = document.createElement("section");
  panel.className = "ui-state-panel";
  panel.dataset.state = options.state;
  panel.setAttribute("role", options.state === "error" || options.state === "permission-denied" ? "alert" : "status");
  panel.setAttribute("aria-live", options.state === "error" ? "assertive" : "polite");
  const title = document.createElement("h2");
  title.textContent = options.title;
  const detail = document.createElement("p");
  detail.textContent = options.detail;
  panel.append(title, detail);
  return panel;
}

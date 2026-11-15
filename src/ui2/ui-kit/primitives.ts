import type { Ui2I18nController } from "../i18n/index.js";
import { createUi2Icon, type Ui2IconName } from "./icons.js";
import { UI2_SETTINGS_MAX_ROWS_PER_CARD } from "./tokens.js";

let nextUi2Uid = 0;
function uid(prefix: string): string {
  nextUi2Uid += 1;
  return `ui2-${prefix}-${nextUi2Uid}`;
}

function isElementHiddenOrInert(el: HTMLElement, boundary: HTMLElement): boolean {
  const view = el.ownerDocument.defaultView;
  let cur: HTMLElement | null = el;
  while (cur) {
    if (
      cur.hidden ||
      cur.hasAttribute("hidden") ||
      cur.hasAttribute("inert") ||
      cur.getAttribute("aria-hidden") === "true"
    ) {
      return true;
    }
    if (cur.style?.display === "none" || cur.style?.visibility === "hidden") {
      return true;
    }
    if (view && typeof view.getComputedStyle === "function") {
      const cs = view.getComputedStyle(cur);
      if (cs.display === "none" || cs.visibility === "hidden") {
        return true;
      }
    }
    if (cur !== el && cur.tagName === "DETAILS" && !(cur as HTMLDetailsElement).open) {
      const directSummary = [...cur.children].find((child) => child.tagName === "SUMMARY");
      if (!directSummary || (el !== directSummary && !directSummary.contains(el))) {
        return true;
      }
    }
    if (cur === boundary) break;
    cur = cur.parentElement;
  }
  return false;
}

function getFocusableElements(container: HTMLElement): HTMLElement[] {
  if (
    container.hidden ||
    container.hasAttribute("hidden") ||
    container.closest("[hidden], [inert], [aria-hidden='true']") ||
    container.style?.display === "none" ||
    container.style?.visibility === "hidden"
  ) {
    return [];
  }
  const candidates = container.querySelectorAll<HTMLElement>(
    'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"]), summary'
  );
  return [...candidates].filter((el) => {
    if ("disabled" in el && (el as HTMLButtonElement).disabled) return false;
    const disabledFieldset = el.closest("fieldset[disabled]");
    if (disabledFieldset) {
      const firstLegend = [...disabledFieldset.children].find((child) => child.tagName === "LEGEND");
      if (!firstLegend || !firstLegend.contains(el)) return false;
    }
    return !isElementHiddenOrInert(el, container);
  });
}

function trapTabFocus(doc: Document, container: HTMLElement, event: KeyboardEvent): void {
  if (event.key !== "Tab") return;
  const focusable = getFocusableElements(container);
  if (focusable.length === 0) {
    event.preventDefault();
    return;
  }
  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  const active =
    doc.activeElement instanceof doc.defaultView!.HTMLElement ? doc.activeElement : null;
  const activeInside = Boolean(active && container.contains(active) && focusable.includes(active));
  if (event.shiftKey) {
    if (!activeInside || active === first) {
      event.preventDefault();
      last.focus();
    }
  } else if (!activeInside || active === last) {
    event.preventDefault();
    first.focus();
  }
}

export interface Ui2ButtonOptions {
  label: string;
  variant?: "primary" | "secondary" | "ghost" | "danger" | "danger-solid";
  size?: "md" | "sm";
  icon?: Ui2IconName;
  type?: "button" | "submit" | "reset";
  disabled?: boolean;
  ariaLabel?: string;
  onClick?: (event: MouseEvent) => void;
}

export function createUi2Button(doc: Document, options: Ui2ButtonOptions): HTMLButtonElement {
  const btn = doc.createElement("button");
  btn.type = options.type ?? "button";
  btn.className = "ui2-btn";
  if (options.variant === "primary") btn.classList.add("ui2-btn-primary");
  else if (options.variant === "ghost") btn.classList.add("ui2-btn-ghost");
  else if (options.variant === "danger") btn.classList.add("ui2-btn-danger");
  else if (options.variant === "danger-solid") btn.classList.add("ui2-btn-danger-solid");
  if (options.size === "sm") btn.classList.add("ui2-btn-sm");
  if (options.disabled) btn.disabled = true;
  if (options.ariaLabel) btn.setAttribute("aria-label", options.ariaLabel);
  if (options.icon) {
    btn.append(createUi2Icon(doc, options.icon, options.size === "sm" ? 14 : 16));
  }
  const span = doc.createElement("span");
  span.textContent = options.label;
  btn.append(span);
  if (options.onClick) {
    btn.addEventListener("click", options.onClick);
  }
  return btn;
}

export interface Ui2ChipOptions {
  id: string;
  label: string;
  pressed?: boolean;
  icon?: Ui2IconName;
  onClick?: () => void;
}

export function createUi2Chip(doc: Document, options: Ui2ChipOptions): HTMLButtonElement {
  const btn = doc.createElement("button");
  btn.type = "button";
  btn.className = "ui2-chip";
  btn.dataset.ui2Chip = options.id;
  btn.setAttribute("aria-pressed", String(Boolean(options.pressed)));
  if (options.icon) {
    btn.append(createUi2Icon(doc, options.icon, 14));
  }
  const label = doc.createElement("span");
  label.textContent = options.label;
  btn.append(label);
  if (options.onClick) {
    btn.addEventListener("click", options.onClick);
  }
  return btn;
}

export interface Ui2ChipGroupItem {
  id: string;
  label: string;
  icon?: Ui2IconName;
}

export interface Ui2ChipGroupOptions {
  ariaLabel: string;
  items: readonly Ui2ChipGroupItem[];
  activeId: string;
  onChange: (id: string) => void;
}

export function createUi2ChipGroup(doc: Document, options: Ui2ChipGroupOptions): HTMLDivElement {
  const group = doc.createElement("div");
  group.className = "ui2-chip-group";
  group.setAttribute("role", "group");
  group.setAttribute("aria-label", options.ariaLabel);
  let active = options.activeId;

  const buttons: HTMLButtonElement[] = [];
  for (const item of options.items) {
    const chip = createUi2Chip(doc, {
      id: item.id,
      label: item.label,
      icon: item.icon,
      pressed: item.id === active,
      onClick: () => {
        active = item.id;
        for (const b of buttons) {
          b.setAttribute("aria-pressed", String(b.dataset.ui2Chip === active));
        }
        options.onChange(active);
      },
    });
    buttons.push(chip);
    group.append(chip);
  }
  return group;
}

export interface Ui2TagOptions {
  label: string;
  tone?: "default" | "ok" | "warn" | "muted";
}

export function createUi2Tag(doc: Document, options: Ui2TagOptions): HTMLSpanElement {
  const tag = doc.createElement("span");
  tag.className = "ui2-tag";
  if (options.tone === "ok") tag.classList.add("ui2-tag-ok");
  else if (options.tone === "warn") tag.classList.add("ui2-tag-warn");
  tag.textContent = options.label;
  return tag;
}

export interface Ui2SwitchOptions {
  ariaLabel: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}

export function createUi2Switch(doc: Document, options: Ui2SwitchOptions): HTMLButtonElement {
  const sw = doc.createElement("button");
  sw.type = "button";
  sw.className = "ui2-switch";
  sw.setAttribute("role", "switch");
  sw.setAttribute("aria-checked", String(options.checked));
  sw.setAttribute("aria-label", options.ariaLabel);
  if (options.disabled) sw.disabled = true;

  const thumb = doc.createElement("span");
  thumb.className = "ui2-switch-thumb";
  thumb.setAttribute("aria-hidden", "true");
  sw.append(thumb);

  let checked = options.checked;
  sw.addEventListener("click", () => {
    checked = !checked;
    sw.setAttribute("aria-checked", String(checked));
    options.onChange(checked);
  });
  return sw;
}

export interface Ui2SegmentedOption<T extends string = string> {
  value: T;
  label: string;
}

export interface Ui2SegmentedOptions<T extends string = string> {
  ariaLabel: string;
  options: readonly Ui2SegmentedOption<T>[];
  value: T;
  onChange: (value: T) => void;
}

export function createUi2Segmented<T extends string = string>(
  doc: Document,
  config: Ui2SegmentedOptions<T>
): HTMLDivElement {
  const group = doc.createElement("div");
  group.className = "ui2-seg";
  group.setAttribute("role", "radiogroup");
  group.setAttribute("aria-label", config.ariaLabel);

  let current = config.value;
  const buttons: HTMLButtonElement[] = [];

  const select = (next: T) => {
    current = next;
    for (const b of buttons) {
      const selected = b.dataset.ui2SegValue === current;
      b.setAttribute("aria-checked", String(selected));
      b.tabIndex = selected ? 0 : -1;
    }
    config.onChange(current);
  };

  config.options.forEach((opt, index) => {
    const btn = doc.createElement("button");
    btn.type = "button";
    btn.className = "ui2-seg-btn";
    btn.setAttribute("role", "radio");
    btn.dataset.ui2SegValue = opt.value;
    const isCurrent = opt.value === current;
    btn.setAttribute("aria-checked", String(isCurrent));
    btn.tabIndex = isCurrent ? 0 : -1;
    btn.textContent = opt.label;
    btn.addEventListener("click", () => select(opt.value));
    btn.addEventListener("keydown", (event) => {
      if (event.key === "ArrowRight" || event.key === "ArrowDown") {
        event.preventDefault();
        const nextIdx = (index + 1) % config.options.length;
        select(config.options[nextIdx].value);
        buttons[nextIdx]?.focus();
      } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
        event.preventDefault();
        const prevIdx = (index - 1 + config.options.length) % config.options.length;
        select(config.options[prevIdx].value);
        buttons[prevIdx]?.focus();
      }
    });
    buttons.push(btn);
    group.append(btn);
  });

  return group;
}

export interface Ui2FieldOptions {
  label: string;
  placeholder?: string;
  value?: string;
  type?: "text" | "search" | "password";
  hint?: string;
  error?: string;
  onInput?: (value: string) => void;
}

export interface Ui2FieldHandle {
  element: HTMLDivElement;
  input: HTMLInputElement;
  setError(error?: string): void;
}

export function createUi2Field(doc: Document, options: Ui2FieldOptions): Ui2FieldHandle {
  const wrap = doc.createElement("div");
  wrap.className = "ui2-field";
  const inputId = uid("field");

  const labelEl = doc.createElement("label");
  labelEl.className = "ui2-field-label";
  labelEl.htmlFor = inputId;
  labelEl.textContent = options.label;

  const input = doc.createElement("input");
  input.id = inputId;
  input.type = options.type ?? "text";
  input.className = "ui2-input";
  if (options.placeholder) input.placeholder = options.placeholder;
  if (options.value !== undefined) input.value = options.value;
  if (options.onInput) {
    input.addEventListener("input", () => options.onInput?.(input.value));
  }

  wrap.append(labelEl, input);

  let hintEl: HTMLParagraphElement | null = null;
  if (options.hint) {
    const hintId = uid("hint");
    hintEl = doc.createElement("p");
    hintEl.id = hintId;
    hintEl.className = "ui2-field-hint";
    hintEl.textContent = options.hint;
    input.setAttribute("aria-describedby", hintId);
    wrap.append(hintEl);
  }

  let errorEl: HTMLParagraphElement | null = null;
  const setError = (err?: string) => {
    if (!err) {
      input.removeAttribute("aria-invalid");
      if (errorEl) {
        errorEl.remove();
        errorEl = null;
      }
      return;
    }
    input.setAttribute("aria-invalid", "true");
    if (!errorEl) {
      errorEl = doc.createElement("p");
      errorEl.id = uid("err");
      errorEl.className = "ui2-field-error";
      errorEl.setAttribute("role", "alert");
      wrap.append(errorEl);
    }
    errorEl.textContent = err;
  };
  if (options.error) setError(options.error);

  return { element: wrap, input, setError };
}

export interface Ui2ListRowOptions {
  title: string;
  meta?: string;
  tag?: Ui2TagOptions;
  icon?: Ui2IconName;
  onClick?: () => void;
  trailing?: HTMLElement;
}

export function createUi2ListRow(doc: Document, options: Ui2ListRowOptions): HTMLElement {
  const row = doc.createElement(options.onClick ? "button" : "div");
  if (row instanceof doc.defaultView!.HTMLButtonElement) {
    row.type = "button";
  }
  row.className = "ui2-list-row";
  if (options.onClick) {
    row.addEventListener("click", options.onClick);
  }

  if (options.icon) {
    row.append(createUi2Icon(doc, options.icon, 16));
  }

  const main = doc.createElement("div");
  main.className = "ui2-list-row-main";
  const titleEl = doc.createElement("span");
  titleEl.className = "ui2-list-row-title";
  titleEl.textContent = options.title;
  main.append(titleEl);

  if (options.meta) {
    const metaEl = doc.createElement("span");
    metaEl.className = "ui2-list-row-meta";
    metaEl.textContent = options.meta;
    main.append(metaEl);
  }
  row.append(main);

  if (options.tag || options.trailing || options.onClick) {
    const end = doc.createElement("div");
    end.className = "ui2-list-row-end";
    if (options.tag) end.append(createUi2Tag(doc, options.tag));
    if (options.trailing) end.append(options.trailing);
    if (options.onClick) end.append(createUi2Icon(doc, "right", 14));
    row.append(end);
  }

  return row;
}

export function createUi2ListCard(doc: Document, rows: readonly HTMLElement[]): HTMLDivElement {
  const card = doc.createElement("div");
  card.className = "ui2-list-card";
  card.append(...rows);
  return card;
}

export interface Ui2CardOptions {
  title?: string;
  subtitle?: string;
  headerAction?: HTMLElement;
}

export interface Ui2CardHandle {
  element: HTMLElement;
  body: HTMLDivElement;
}

export function createUi2Card(doc: Document, options: Ui2CardOptions = {}): Ui2CardHandle {
  const section = doc.createElement("section");
  section.className = "ui2-card";

  if (options.title || options.subtitle || options.headerAction) {
    const head = doc.createElement("div");
    head.className = "ui2-card-head";
    const copy = doc.createElement("div");
    copy.className = "ui2-stack-sm";
    if (options.title) {
      const h = doc.createElement("h3");
      h.className = "ui2-h3";
      h.textContent = options.title;
      copy.append(h);
    }
    if (options.subtitle) {
      const p = doc.createElement("p");
      p.className = "ui2-field-hint";
      p.textContent = options.subtitle;
      copy.append(p);
    }
    head.append(copy);
    if (options.headerAction) head.append(options.headerAction);
    section.append(head);
  }

  const body = doc.createElement("div");
  body.className = "ui2-stack";
  section.append(body);
  return { element: section, body };
}

export interface Ui2StatCardOptions {
  label: string;
  value: string;
}

export function createUi2StatCard(doc: Document, options: Ui2StatCardOptions): HTMLElement {
  const card = doc.createElement("div");
  card.className = "ui2-stat-card";
  const val = doc.createElement("div");
  val.className = "ui2-stat-value";
  val.textContent = options.value;
  const label = doc.createElement("div");
  label.className = "ui2-stat-label";
  label.textContent = options.label;
  card.append(val, label);
  return card;
}

export interface Ui2MemoryCardOptions {
  statement: string;
  meta: string;
  statusTag: Ui2TagOptions;
  translatedFromLabel?: string | null;
  toggleOriginalLabel?: string | null;
  onToggleOriginal?: () => void;
  actionLabel?: string;
  onInspect?: () => void;
}

export function createUi2MemoryCard(doc: Document, options: Ui2MemoryCardOptions): HTMLElement {
  const card = doc.createElement("article");
  card.className = "ui2-mem-card";

  const statement = doc.createElement("p");
  statement.className = "ui2-mem-statement";
  statement.textContent = options.statement;
  card.append(statement);

  if (options.translatedFromLabel) {
    const transRow = doc.createElement("div");
    transRow.className = "ui2-row-flex";
    const note = createUi2Tag(doc, { label: options.translatedFromLabel, tone: "muted" });
    transRow.append(note);
    if (options.toggleOriginalLabel && options.onToggleOriginal) {
      transRow.append(
        createUi2Button(doc, {
          label: options.toggleOriginalLabel,
          variant: "ghost",
          size: "sm",
          onClick: options.onToggleOriginal,
        })
      );
    }
    card.append(transRow);
  }

  const footer = doc.createElement("div");
  footer.className = "ui2-mem-footer";
  const left = doc.createElement("div");
  left.className = "ui2-row-flex";
  left.append(createUi2Tag(doc, options.statusTag));
  const metaEl = doc.createElement("span");
  metaEl.className = "ui2-list-row-meta";
  metaEl.textContent = options.meta;
  left.append(metaEl);
  footer.append(left);

  if (options.actionLabel && options.onInspect) {
    footer.append(
      createUi2Button(doc, {
        label: options.actionLabel,
        variant: "ghost",
        size: "sm",
        icon: "right",
        onClick: options.onInspect,
      })
    );
  }
  card.append(footer);
  return card;
}

export interface Ui2EmptyStateOptions {
  icon: Ui2IconName;
  title: string;
  body: string;
  actionLabel?: string;
  onAction?: () => void;
}

export function createUi2EmptyState(doc: Document, options: Ui2EmptyStateOptions): HTMLElement {
  const box = doc.createElement("div");
  box.className = "ui2-empty";
  box.append(createUi2Icon(doc, options.icon, 24));
  const title = doc.createElement("h3");
  title.className = "ui2-empty-title";
  title.textContent = options.title;
  const body = doc.createElement("p");
  body.className = "ui2-empty-body";
  body.textContent = options.body;
  box.append(title, body);
  if (options.actionLabel && options.onAction) {
    box.append(
      createUi2Button(doc, {
        label: options.actionLabel,
        variant: "secondary",
        size: "sm",
        onClick: options.onAction,
      })
    );
  }
  return box;
}

export interface Ui2BannerOptions {
  tone?: "neutral" | "warn" | "ok" | "danger";
  title: string;
  message: string;
  actionLabel?: string;
  onAction?: () => void;
}

export function createUi2Banner(doc: Document, options: Ui2BannerOptions): HTMLElement {
  const banner = doc.createElement("div");
  banner.className = "ui2-banner";
  if (options.tone === "warn") banner.classList.add("ui2-banner-warn");
  else if (options.tone === "ok") banner.classList.add("ui2-banner-ok");
  else if (options.tone === "danger") banner.classList.add("ui2-banner-danger");
  banner.setAttribute("role", options.tone === "danger" || options.tone === "warn" ? "alert" : "status");

  const body = doc.createElement("div");
  body.className = "ui2-banner-body";
  const titleEl = doc.createElement("p");
  titleEl.className = "ui2-banner-title";
  titleEl.textContent = options.title;
  const msgEl = doc.createElement("p");
  msgEl.className = "ui2-banner-text";
  msgEl.textContent = options.message;
  body.append(titleEl, msgEl);
  banner.append(body);

  if (options.actionLabel && options.onAction) {
    banner.append(
      createUi2Button(doc, {
        label: options.actionLabel,
        variant: "secondary",
        size: "sm",
        onClick: options.onAction,
      })
    );
  }
  return banner;
}

export interface Ui2InlineConfirmOptions {
  triggerLabel: string;
  question: string;
  confirmLabel: string;
  cancelLabel: string;
  onConfirm: () => void;
}

export interface Ui2InlineConfirmHandle {
  element: HTMLDivElement;
  trigger: HTMLButtonElement;
  isOpen(): boolean;
  cancel(): void;
}

export function createUi2InlineConfirm(
  doc: Document,
  options: Ui2InlineConfirmOptions
): Ui2InlineConfirmHandle {
  const wrap = doc.createElement("div");
  wrap.className = "ui2-inline-confirm";
  let open = false;

  const box = doc.createElement("div");
  box.className = "ui2-inline-confirm-box";
  box.hidden = true;

  const questionEl = doc.createElement("span");
  questionEl.className = "ui2-banner-text";
  questionEl.textContent = options.question;

  const actions = doc.createElement("div");
  actions.className = "ui2-row-flex";

  const trigger = createUi2Button(doc, {
    label: options.triggerLabel,
    variant: "danger",
    size: "sm",
    onClick: () => {
      open = true;
      box.hidden = false;
      trigger.hidden = true;
      cancelBtn.focus();
    },
  });

  const cancelBtn = createUi2Button(doc, {
    label: options.cancelLabel,
    variant: "secondary",
    size: "sm",
    onClick: () => {
      open = false;
      box.hidden = true;
      trigger.hidden = false;
      trigger.focus();
    },
  });

  const confirmBtn = createUi2Button(doc, {
    label: options.confirmLabel,
    variant: "danger-solid",
    size: "sm",
    onClick: () => {
      open = false;
      box.hidden = true;
      trigger.hidden = false;
      options.onConfirm();
      trigger.focus();
    },
  });

  actions.append(cancelBtn, confirmBtn);
  box.append(questionEl, actions);
  wrap.append(trigger, box);

  return {
    element: wrap,
    trigger,
    isOpen: () => open,
    cancel: () => {
      open = false;
      box.hidden = true;
      trigger.hidden = false;
    },
  };
}

export interface Ui2DialogOptions {
  title: string;
  closeLabel: string;
  onClose?: () => void;
}

export interface Ui2DialogHandle {
  backdrop: HTMLDivElement;
  dialog: HTMLDivElement;
  body: HTMLDivElement;
  footer: HTMLDivElement;
  open(opener?: HTMLElement | null): void;
  close(): void;
  isOpen(): boolean;
  destroy(): void;
}

export function createUi2Dialog(doc: Document, options: Ui2DialogOptions): Ui2DialogHandle {
  const backdrop = doc.createElement("div");
  backdrop.className = "ui2-backdrop";
  backdrop.hidden = true;

  const dialog = doc.createElement("div");
  dialog.className = "ui2-dialog";
  dialog.setAttribute("role", "dialog");
  dialog.setAttribute("aria-modal", "true");
  const titleId = uid("dlg-title");
  dialog.setAttribute("aria-labelledby", titleId);

  const head = doc.createElement("div");
  head.className = "ui2-dialog-head";
  const heading = doc.createElement("h2");
  heading.id = titleId;
  heading.className = "ui2-h2";
  heading.textContent = options.title;
  const closeBtn = createUi2Button(doc, {
    label: options.closeLabel,
    variant: "ghost",
    size: "sm",
    icon: "x",
    ariaLabel: options.closeLabel,
    onClick: () => close(),
  });
  head.append(heading, closeBtn);

  const body = doc.createElement("div");
  body.className = "ui2-stack";
  const footer = doc.createElement("div");
  footer.className = "ui2-row-flex";

  dialog.append(head, body, footer);
  backdrop.append(dialog);

  let opened = false;
  let lastOpener: HTMLElement | null = null;

  const handleKeyDown = (event: KeyboardEvent) => {
    if (!opened) return;
    if (event.key === "Escape") {
      event.preventDefault();
      close();
      return;
    }
    trapTabFocus(doc, dialog, event);
  };

  backdrop.addEventListener("click", (event) => {
    if (event.target === backdrop) close();
  });
  dialog.addEventListener("keydown", handleKeyDown);

  const open = (opener?: HTMLElement | null) => {
    lastOpener =
      opener ?? (doc.activeElement instanceof doc.defaultView!.HTMLElement ? doc.activeElement : null);
    opened = true;
    backdrop.hidden = false;
    const focusable = getFocusableElements(dialog);
    (focusable[0] ?? closeBtn).focus();
  };

  const close = () => {
    if (!opened) return;
    opened = false;
    backdrop.hidden = true;
    options.onClose?.();
    if (lastOpener && lastOpener.isConnected) {
      lastOpener.focus();
    }
    lastOpener = null;
  };

  return {
    backdrop,
    dialog,
    body,
    footer,
    open,
    close,
    isOpen: () => opened,
    destroy: () => {
      opened = false;
      backdrop.remove();
    },
  };
}

export interface Ui2AsideOptions {
  title: string;
  closeLabel: string;
  onClose?: () => void;
}

export interface Ui2AsideHandle {
  element: HTMLElement;
  body: HTMLDivElement;
  open(opener?: HTMLElement | null): void;
  close(): void;
  isOpen(): boolean;
}

export function createUi2Aside(doc: Document, options: Ui2AsideOptions): Ui2AsideHandle {
  const aside = doc.createElement("aside");
  aside.className = "ui2-aside";
  aside.setAttribute("role", "dialog");
  aside.setAttribute("aria-modal", "false");
  const titleId = uid("aside-title");
  aside.setAttribute("aria-labelledby", titleId);
  aside.hidden = true;

  const head = doc.createElement("div");
  head.className = "ui2-card-head";
  const heading = doc.createElement("h3");
  heading.id = titleId;
  heading.className = "ui2-h3";
  heading.textContent = options.title;
  const closeBtn = createUi2Button(doc, {
    label: options.closeLabel,
    variant: "ghost",
    size: "sm",
    icon: "x",
    ariaLabel: options.closeLabel,
    onClick: () => close(),
  });
  head.append(heading, closeBtn);

  const body = doc.createElement("div");
  body.className = "ui2-stack";
  aside.append(head, body);

  let opened = false;
  let lastOpener: HTMLElement | null = null;

  aside.addEventListener("keydown", (event) => {
    if (!opened) return;
    if (event.key === "Escape") {
      event.preventDefault();
      close();
      return;
    }
    trapTabFocus(doc, aside, event);
  });

  const open = (opener?: HTMLElement | null) => {
    lastOpener =
      opener ?? (doc.activeElement instanceof doc.defaultView!.HTMLElement ? doc.activeElement : null);
    opened = true;
    aside.hidden = false;
    const focusable = getFocusableElements(aside);
    (focusable[0] ?? closeBtn).focus();
  };

  const close = () => {
    if (!opened) return;
    opened = false;
    aside.hidden = true;
    options.onClose?.();
    if (lastOpener && lastOpener.isConnected) {
      lastOpener.focus();
    }
    lastOpener = null;
  };

  return {
    element: aside,
    body,
    open,
    close,
    isOpen: () => opened,
  };
}

/**
 * Renders technical identifiers only inside a hidden-by-default Details surface.
 */
export function renderUi2TechnicalCode(doc: Document, value: string): HTMLElement {
  const code = doc.createElement("code");
  code.className = "ui2-tech-code";
  code.dataset.ui2TechnicalCode = "true";
  code.textContent = value;
  return code;
}

export interface Ui2TechnicalEntry {
  label: string;
  value: string;
}

export interface Ui2TechnicalDetailsOptions {
  summaryLabel: string;
  entries: readonly Ui2TechnicalEntry[];
  defaultOpen?: boolean;
}

export function createUi2TechnicalDetails(
  doc: Document,
  options: Ui2TechnicalDetailsOptions
): HTMLDetailsElement {
  const details = doc.createElement("details");
  details.className = "ui2-tech-details";
  details.dataset.ui2TechnicalDetails = "true";
  details.open = Boolean(options.defaultOpen);

  const summary = doc.createElement("summary");
  summary.className = "ui2-tech-summary";
  summary.textContent = options.summaryLabel;
  details.append(summary);

  const list = doc.createElement("dl");
  list.className = "ui2-tech-list";
  for (const entry of options.entries) {
    const row = doc.createElement("div");
    row.className = "ui2-tech-row";
    const dt = doc.createElement("dt");
    dt.className = "ui2-tech-key";
    dt.textContent = entry.label;
    const dd = doc.createElement("dd");
    dd.style.margin = "0";
    dd.append(renderUi2TechnicalCode(doc, entry.value));
    row.append(dt, dd);
    list.append(row);
  }
  details.append(list);
  return details;
}

export interface Ui2BreadcrumbItem {
  id: string;
  label: string;
  onClick?: () => void;
}

export interface Ui2BreadcrumbOptions {
  ariaLabel: string;
  items: readonly Ui2BreadcrumbItem[];
}

export function createUi2Breadcrumb(doc: Document, options: Ui2BreadcrumbOptions): HTMLElement {
  const nav = doc.createElement("nav");
  nav.className = "ui2-breadcrumb";
  nav.setAttribute("aria-label", options.ariaLabel);

  const ol = doc.createElement("ol");
  ol.className = "ui2-breadcrumb-list";
  const lastIndex = options.items.length - 1;

  options.items.forEach((item, index) => {
    const li = doc.createElement("li");
    li.className = "ui2-breadcrumb-item";
    const isLast = index === lastIndex;

    if (isLast || !item.onClick) {
      const span = doc.createElement("span");
      span.className = "ui2-breadcrumb-current";
      if (isLast) span.setAttribute("aria-current", "page");
      span.textContent = item.label;
      li.append(span);
    } else {
      const btn = doc.createElement("button");
      btn.type = "button";
      btn.className = "ui2-breadcrumb-link";
      btn.textContent = item.label;
      btn.addEventListener("click", item.onClick);
      li.append(btn);
    }

    if (!isLast) {
      const sep = doc.createElement("span");
      sep.setAttribute("aria-hidden", "true");
      sep.append(createUi2Icon(doc, "right", 14));
      li.append(sep);
    }
    ol.append(li);
  });

  nav.append(ol);
  return nav;
}

export interface Ui2ToastHandle {
  element: HTMLDivElement;
  show(message: string): void;
  hide(): void;
}

export function createUi2Toast(doc: Document): Ui2ToastHandle {
  const el = doc.createElement("div");
  el.className = "ui2-toast";
  el.setAttribute("role", "status");
  el.setAttribute("aria-live", "polite");
  el.hidden = true;

  const text = doc.createElement("span");
  el.append(createUi2Icon(doc, "check", 16), text);

  return {
    element: el,
    show(message: string) {
      text.textContent = message;
      el.hidden = false;
    },
    hide() {
      el.hidden = true;
    },
  };
}

export interface Ui2SettingsRowOptions {
  title: string;
  description?: string;
  control: HTMLElement;
}

export function createUi2SettingsRow(doc: Document, options: Ui2SettingsRowOptions): HTMLDivElement {
  const row = doc.createElement("div");
  row.className = "ui2-settings-row";

  const copy = doc.createElement("div");
  copy.className = "ui2-settings-copy";
  const titleEl = doc.createElement("span");
  titleEl.className = "ui2-settings-title";
  titleEl.textContent = options.title;
  copy.append(titleEl);
  if (options.description) {
    const descEl = doc.createElement("span");
    descEl.className = "ui2-settings-desc";
    descEl.textContent = options.description;
    copy.append(descEl);
  }

  row.append(copy, options.control);
  return row;
}

export interface Ui2SettingsCardOptions {
  title: string;
  rows: readonly Ui2SettingsRowOptions[];
}

export function createUi2SettingsCard(doc: Document, options: Ui2SettingsCardOptions): HTMLElement {
  if (options.rows.length > UI2_SETTINGS_MAX_ROWS_PER_CARD) {
    throw new Error(
      `MP_UI2_SETTINGS_ROW_CAP_EXCEEDED: Settings card allows at most ${UI2_SETTINGS_MAX_ROWS_PER_CARD} rows (got ${options.rows.length})`
    );
  }
  const card = doc.createElement("section");
  card.className = "ui2-settings-card";

  const head = doc.createElement("div");
  head.className = "ui2-settings-head";
  const titleEl = doc.createElement("h3");
  titleEl.className = "ui2-h3";
  titleEl.textContent = options.title;
  head.append(titleEl);
  card.append(head);

  for (const row of options.rows) {
    card.append(createUi2SettingsRow(doc, row));
  }
  return card;
}

export interface Ui2DisclosureOptions {
  summaryLabel: string;
  body: string | HTMLElement;
  defaultOpen?: boolean;
}

export function createUi2Disclosure(
  doc: Document,
  options: Ui2DisclosureOptions
): HTMLDetailsElement {
  const details = doc.createElement("details");
  details.className = "ui2-disclosure";
  details.open = Boolean(options.defaultOpen);

  const summary = doc.createElement("summary");
  summary.className = "ui2-disclosure-summary";
  summary.append(createUi2Icon(doc, "chev", 14));
  const label = doc.createElement("span");
  label.textContent = options.summaryLabel;
  summary.append(label);
  details.append(summary);

  const body = doc.createElement("div");
  body.className = "ui2-disclosure-body";
  if (typeof options.body === "string") {
    body.textContent = options.body;
  } else {
    body.append(options.body);
  }
  details.append(body);
  return details;
}

export interface Ui2ChatMessageOptions {
  role: "user" | "assistant";
  roleLabel: string;
  timestampLabel: string;
  text: string;
}

export function createUi2ChatMessage(doc: Document, options: Ui2ChatMessageOptions): HTMLElement {
  const msg = doc.createElement("article");
  msg.className = "ui2-chat-msg";
  if (options.role === "user") msg.classList.add("ui2-chat-msg-user");

  const meta = doc.createElement("div");
  meta.className = "ui2-chat-meta";
  meta.textContent = `${options.roleLabel} · ${options.timestampLabel}`;

  const body = doc.createElement("p");
  body.className = "ui2-chat-body";
  body.textContent = options.text;

  msg.append(meta, body);
  return msg;
}

export function createUi2LoadingSkeleton(doc: Document, ariaLabel: string, i18n?: Ui2I18nController): HTMLElement {
  const wrap = doc.createElement("div");
  wrap.className = "ui2-skeleton";
  wrap.setAttribute("role", "status");
  wrap.setAttribute("aria-busy", "true");
  wrap.setAttribute("aria-label", i18n ? i18n.t("state.loading") : ariaLabel);

  const l1 = doc.createElement("div");
  l1.className = "ui2-skeleton-line";
  const l2 = doc.createElement("div");
  l2.className = "ui2-skeleton-line ui2-skeleton-line-short";
  const label = doc.createElement("span");
  label.className = "ui2-field-hint";
  label.textContent = i18n ? i18n.t("state.loadingHint") : ariaLabel;

  wrap.append(l1, l2, label);
  return wrap;
}

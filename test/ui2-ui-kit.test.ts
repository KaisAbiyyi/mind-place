import assert from "node:assert/strict";
import test from "node:test";
import { JSDOM } from "jsdom";
import { createI18nController } from "../src/ui2/i18n/index.js";
import {
  UI2_DESTINATIONS,
  UI2_FONT_SIZES_PX,
  UI2_FONT_WEIGHTS,
  UI2_ICON_NAMES,
  UI2_MAP_NAMES,
  UI2_RADII_PX,
  UI2_SETTINGS_MAX_ROWS_PER_CARD,
  UI2_SPACING_PX,
  createUi2Aside,
  createUi2Banner,
  createUi2Breadcrumb,
  createUi2Button,
  createUi2Card,
  createUi2ChatMessage,
  createUi2ChipGroup,
  createUi2Dialog,
  createUi2Disclosure,
  createUi2EmptyState,
  createUi2Field,
  createUi2Icon,
  createUi2InlineConfirm,
  createUi2ListCard,
  createUi2ListRow,
  createUi2MemoryCard,
  createUi2Segmented,
  createUi2SettingsCard,
  createUi2StatCard,
  createUi2Switch,
  createUi2Tag,
  createUi2TechnicalDetails,
  createUi2Toast,
  ensureUi2IconSprite,
  mountUi2Gallery,
} from "../src/ui2/ui-kit/index.js";

test("MP-UI2-03: Token scales and canonical destination order match UI-PRINCIPLES.md", () => {
  assert.deepEqual(UI2_FONT_SIZES_PX, [12, 13, 14, 16, 20, 28]);
  assert.deepEqual(UI2_FONT_WEIGHTS, [400, 500, 600]);
  assert.deepEqual(UI2_SPACING_PX, [4, 8, 12, 16, 24, 32, 48]);
  assert.deepEqual(UI2_RADII_PX, [7, 10, 14]);
  assert.deepEqual(UI2_DESTINATIONS, [
    "home",
    "memory",
    "archive",
    "constellation",
    "sources",
    "connections",
    "settings",
  ]);
  assert.deepEqual(UI2_MAP_NAMES, ["Constellation", "Atlas", "Mindscape", "Synapse", "Hive"]);
  assert.equal(UI2_SETTINGS_MAX_ROWS_PER_CARD, 7);
});

test("MP-UI2-03: Icon sprite injects once cleanly and includes all 23 wireframe icons", () => {
  const dom = new JSDOM(`<!DOCTYPE html><html><body></body></html>`);
  const doc = dom.window.document;
  assert.equal(UI2_ICON_NAMES.length, 23);

  const sprite1 = ensureUi2IconSprite(doc);
  const sprite2 = ensureUi2IconSprite(doc);
  assert.equal(sprite1, sprite2);
  assert.equal(sprite1.querySelectorAll("symbol").length, 23);

  const icon = createUi2Icon(doc, "memory", 16);
  assert.equal(icon.getAttribute("aria-hidden"), "true");
  assert.equal(icon.querySelector("use")?.getAttribute("href"), "#ui2-i-memory");
});

test("MP-UI2-03: Controls, focus restoration, Escape close, inline confirm, and Settings row cap", () => {
  const dom = new JSDOM(`<!DOCTYPE html><html><body><div id="mount"></div></body></html>`);
  const doc = dom.window.document;
  const mount = doc.getElementById("mount")!;
  const win = dom.window;

  // Chip group
  let chipValue = "all";
  const chips = createUi2ChipGroup(doc, {
    ariaLabel: "Filters",
    activeId: "all",
    items: [
      { id: "all", label: "All" },
      { id: "chatgpt", label: "ChatGPT" },
    ],
    onChange: (id) => {
      chipValue = id;
    },
  });
  mount.append(chips);
  const chatgptChip = chips.querySelector<HTMLButtonElement>('[data-ui2-chip="chatgpt"]')!;
  chatgptChip.click();
  assert.equal(chipValue, "chatgpt");
  assert.equal(chatgptChip.getAttribute("aria-pressed"), "true");

  // Switch & Segmented keyboard navigation
  let swVal = false;
  const sw = createUi2Switch(doc, {
    ariaLabel: "Auto save",
    checked: false,
    onChange: (v) => {
      swVal = v;
    },
  });
  sw.click();
  assert.equal(swVal, true);
  assert.equal(sw.getAttribute("aria-checked"), "true");

  let segVal = "en";
  const seg = createUi2Segmented(doc, {
    ariaLabel: "Language",
    options: [
      { value: "en", label: "EN" },
      { value: "id", label: "ID" },
    ],
    value: "en",
    onChange: (v) => {
      segVal = v;
    },
  });
  mount.append(seg);
  const firstSeg = seg.querySelector<HTMLButtonElement>('[data-ui2-seg-value="en"]')!;
  firstSeg.focus();
  firstSeg.dispatchEvent(new win.KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
  assert.equal(segVal, "id");

  // Field error + Inline confirm (no window.confirm)
  const field = createUi2Field(doc, { label: "Passphrase" });
  field.setError("Too short");
  assert.equal(field.input.getAttribute("aria-invalid"), "true");
  field.setError();
  assert.equal(field.input.getAttribute("aria-invalid"), null);

  let confirmed = false;
  const inline = createUi2InlineConfirm(doc, {
    triggerLabel: "Remove",
    question: "Remove item?",
    confirmLabel: "Confirm",
    cancelLabel: "Cancel",
    onConfirm: () => {
      confirmed = true;
    },
  });
  mount.append(inline.element);
  inline.trigger.click();
  assert.equal(inline.isOpen(), true);
  const confirmBtn = inline.element.querySelector<HTMLButtonElement>(".ui2-btn-danger-solid")!;
  confirmBtn.click();
  assert.equal(confirmed, true);
  assert.equal(inline.isOpen(), false);

  // Dialog Escape + focus restoration
  const opener = createUi2Button(doc, { label: "Open dialog" });
  mount.append(opener);
  const dlg = createUi2Dialog(doc, { title: "Review", closeLabel: "Close" });
  dlg.body.append(createUi2Tag(doc, { label: "Approved", tone: "ok" }));
  dlg.footer.append(createUi2Button(doc, { label: "Done", onClick: () => dlg.close() }));
  mount.append(dlg.backdrop);

  opener.focus();
  dlg.open(opener);
  assert.equal(dlg.isOpen(), true);
  assert.equal(dlg.dialog.getAttribute("aria-modal"), "true");
  dlg.dialog.dispatchEvent(new win.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  assert.equal(dlg.isOpen(), false);
  assert.equal(doc.activeElement, opener);

  // Aside Escape + focus restoration
  const asideOpener = createUi2Button(doc, { label: "Open aside" });
  mount.append(asideOpener);
  const aside = createUi2Aside(doc, { title: "Aside", closeLabel: "Close" });
  mount.append(aside.element);
  asideOpener.focus();
  aside.open(asideOpener);
  assert.equal(aside.isOpen(), true);
  aside.element.dispatchEvent(new win.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  assert.equal(aside.isOpen(), false);
  assert.equal(doc.activeElement, asideOpener);

  // Technical details collapsed by default + Toast + Breadcrumb + Disclosure + Chat + Card
  const tech = createUi2TechnicalDetails(doc, {
    summaryLabel: "Details",
    entries: [{ label: "Hash", value: "sha256:abcdef12" }],
  });
  assert.equal(tech.open, false);
  assert.equal(tech.dataset.ui2TechnicalDetails, "true");

  const toast = createUi2Toast(doc);
  toast.show("Saved");
  assert.equal(toast.element.hidden, false);

  let crumbClicked = "";
  const crumb = createUi2Breadcrumb(doc, {
    ariaLabel: "Breadcrumb",
    items: [
      { id: "home", label: "Home", onClick: () => { crumbClicked = "home"; } },
      { id: "current", label: "Memory" },
    ],
  });
  crumb.querySelector<HTMLButtonElement>("button")?.click();
  assert.equal(crumbClicked, "home");
  assert.equal(crumb.querySelector('[aria-current="page"]')?.textContent, "Memory");

  const rowCard = createUi2ListCard(doc, [createUi2ListRow(doc, { title: "Notes", meta: "Today" })]);
  const memCard = createUi2MemoryCard(doc, {
    statement: "Decision recorded.",
    meta: "Today",
    statusTag: { label: "Current", tone: "ok" },
  });
  const statCard = createUi2StatCard(doc, { label: "Chats", value: "12" });
  const card = createUi2Card(doc, { title: "Box" });
  card.body.append(
    rowCard,
    memCard,
    statCard,
    createUi2EmptyState(doc, { icon: "memory", title: "Empty", body: "Nothing yet" }),
    createUi2Banner(doc, { tone: "warn", title: "Partial", message: "Partial save" }),
    createUi2Disclosure(doc, { summaryLabel: "More", body: "Text" }),
    createUi2ChatMessage(doc, { role: "user", roleLabel: "You", timestampLabel: "Now", text: "Hi" })
  );
  mount.append(tech, toast.element, crumb, card.element);

  // Settings 7-row hard cap enforcement
  const makeRow = (n: number) => ({
    title: `Row ${n}`,
    control: createUi2Button(doc, { label: "Action" }),
  });
  assert.doesNotThrow(() =>
    createUi2SettingsCard(doc, {
      title: "Valid 7 rows",
      rows: [1, 2, 3, 4, 5, 6, 7].map(makeRow),
    })
  );
  assert.throws(
    () =>
      createUi2SettingsCard(doc, {
        title: "Too many rows",
        rows: [1, 2, 3, 4, 5, 6, 7, 8].map(makeRow),
      }),
    /MP_UI2_SETTINGS_ROW_CAP_EXCEEDED/
  );
});

test("MP-UI2-03: Gallery renders in EN and ID and toggles theme/density cleanly", () => {
  const dom = new JSDOM(`<!DOCTYPE html><html><body><div id="app"></div></body></html>`);
  const doc = dom.window.document;
  const app = doc.getElementById("app")!;
  const i18n = createI18nController({ initialLocale: "en" });

  const gallery = mountUi2Gallery(app, { i18n, initialTheme: "light", initialDensity: "comfortable" });
  assert.equal(doc.documentElement.dataset.theme, "light");
  assert.equal(doc.documentElement.dataset.density, "comfortable");
  assert.equal(doc.querySelector("h1")?.textContent, "Design system v2 gallery");

  const tech = doc.querySelector<HTMLDetailsElement>('[data-ui2-technical-details="true"]')!;
  assert.equal(tech.open, false);

  i18n.setLocale("id");
  assert.equal(doc.querySelector("h1")?.textContent, "Galeri sistem desain v2");
  gallery.setTheme("dark");
  assert.equal(doc.documentElement.dataset.theme, "dark");
  gallery.setDensity("compact");
  assert.equal(doc.documentElement.dataset.density, "compact");

  gallery.destroy();
  assert.equal(app.childElementCount, 0);
});

test("MP-UI2-03: Dialog and Aside trap forward/reverse Tab focus and exclude hidden or collapsed subtrees", () => {
  const dom = new JSDOM(`<!DOCTYPE html><html><body><div id="mount"></div></body></html>`);
  const doc = dom.window.document;
  const win = dom.window;
  const mount = doc.getElementById("mount")!;

  const dispatchTab = (target: HTMLElement, shiftKey = false) => {
    const ev = new win.KeyboardEvent("keydown", {
      key: "Tab",
      shiftKey,
      bubbles: true,
      cancelable: true,
    });
    target.dispatchEvent(ev);
    return ev;
  };

  // 1. Aside focus trap, hidden-ancestor exclusion, collapsed details, and Escape restore
  const asideOpener = createUi2Button(doc, { label: "Open aside" });
  mount.append(asideOpener);
  const aside = createUi2Aside(doc, { title: "Aside", closeLabel: "Close" });
  const asideCloseBtn = aside.element.querySelector<HTMLButtonElement>("button")!;

  const hiddenWrap = doc.createElement("div");
  hiddenWrap.hidden = true;
  const hiddenBtn = createUi2Button(doc, { label: "Hidden child" });
  hiddenWrap.append(hiddenBtn);

  const ariaHiddenWrap = doc.createElement("div");
  ariaHiddenWrap.setAttribute("aria-hidden", "true");
  const ariaHiddenBtn = createUi2Button(doc, { label: "Aria hidden child" });
  ariaHiddenWrap.append(ariaHiddenBtn);

  const displayNoneWrap = doc.createElement("div");
  displayNoneWrap.style.display = "none";
  const displayNoneBtn = createUi2Button(doc, { label: "Display none child" });
  displayNoneWrap.append(displayNoneBtn);

  const details = doc.createElement("details");
  const summary = doc.createElement("summary");
  summary.textContent = "More";
  const detailsInnerBtn = createUi2Button(doc, { label: "Details action" });
  details.append(summary, detailsInnerBtn);

  const asideLastBtn = createUi2Button(doc, { label: "Open item" });

  aside.body.append(hiddenWrap, ariaHiddenWrap, displayNoneWrap, details, asideLastBtn);
  mount.append(aside.element);

  // Closed aside must never trap or steal focus
  asideOpener.focus();
  const closedTabEvent = dispatchTab(aside.element, false);
  assert.equal(closedTabEvent.defaultPrevented, false);
  assert.equal(doc.activeElement, asideOpener);

  // Open aside -> first focusable control (closeBtn) receives focus
  aside.open(asideOpener);
  assert.equal(aside.isOpen(), true);
  assert.equal(doc.activeElement, asideCloseBtn);

  // Shift+Tab from first wraps to last visible control (asideLastBtn), skipping hidden/collapsed controls
  const revFromFirst = dispatchTab(asideCloseBtn, true);
  assert.equal(revFromFirst.defaultPrevented, true);
  assert.equal(doc.activeElement, asideLastBtn);

  // Tab from last visible control wraps back to first (asideCloseBtn)
  const fwdFromLast = dispatchTab(asideLastBtn, false);
  assert.equal(fwdFromLast.defaultPrevented, true);
  assert.equal(doc.activeElement, asideCloseBtn);

  // Hide asideLastBtn via hidden wrapper -> summary becomes last, while closed detailsInnerBtn stays unreachable
  asideLastBtn.hidden = true;
  dispatchTab(asideCloseBtn, true);
  assert.equal(doc.activeElement, summary);
  assert.notEqual(doc.activeElement, detailsInnerBtn);

  dispatchTab(summary, false);
  assert.equal(doc.activeElement, asideCloseBtn);

  // Opening <details> makes detailsInnerBtn reachable as last focusable element
  details.open = true;
  dispatchTab(asideCloseBtn, true);
  assert.equal(doc.activeElement, detailsInnerBtn);
  dispatchTab(detailsInnerBtn, false);
  assert.equal(doc.activeElement, asideCloseBtn);

  // Escape closes aside and restores opener focus; closed aside no longer traps
  aside.element.dispatchEvent(new win.KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
  assert.equal(aside.isOpen(), false);
  assert.equal(aside.element.hidden, true);
  assert.equal(doc.activeElement, asideOpener);
  const afterCloseTab = dispatchTab(aside.element, false);
  assert.equal(afterCloseTab.defaultPrevented, false);
  assert.equal(doc.activeElement, asideOpener);

  // 2. Dialog hidden-descendant exclusion on open() and Tab / Shift+Tab wrapping
  const dlgOpener = createUi2Button(doc, { label: "Open dialog" });
  mount.append(dlgOpener);
  const dlg = createUi2Dialog(doc, { title: "Dialog", closeLabel: "Close" });
  const dlgCloseBtn = dlg.dialog.querySelector<HTMLButtonElement>("button")!;

  const dlgHiddenFirstWrap = doc.createElement("div");
  dlgHiddenFirstWrap.hidden = true;
  const dlgHiddenFirstBtn = createUi2Button(doc, { label: "Hidden first" });
  dlgHiddenFirstWrap.append(dlgHiddenFirstBtn);

  const dlgDoneBtn = createUi2Button(doc, { label: "Done" });
  const dlgHiddenLastWrap = doc.createElement("div");
  dlgHiddenLastWrap.style.visibility = "hidden";
  const dlgHiddenLastBtn = createUi2Button(doc, { label: "Hidden last" });
  dlgHiddenLastWrap.append(dlgHiddenLastBtn);

  dlg.body.append(dlgHiddenFirstWrap);
  dlg.footer.append(dlgDoneBtn, dlgHiddenLastWrap);
  mount.append(dlg.backdrop);

  dlgOpener.focus();
  dlg.open(dlgOpener);
  assert.equal(doc.activeElement, dlgCloseBtn);

  dispatchTab(dlgCloseBtn, true);
  assert.equal(doc.activeElement, dlgDoneBtn);

  dispatchTab(dlgDoneBtn, false);
  assert.equal(doc.activeElement, dlgCloseBtn);

  dlg.dialog.dispatchEvent(new win.KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
  assert.equal(dlg.isOpen(), false);
  assert.equal(doc.activeElement, dlgOpener);
});

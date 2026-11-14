import assert from "node:assert/strict";
import test from "node:test";
import { JSDOM } from "jsdom";
import {
  EN_CATALOG,
  ID_CATALOG,
  PROTECTED_PRODUCT_NAMES,
  UI2_LOCALE_STORAGE_KEY,
  createI18nController,
  formatDate,
  formatNumber,
  formatUserChatText,
  resolveCuratedMemoryCopy,
  t,
  tPlural,
  toPseudoLocale,
  validateCatalogParity,
} from "../src/ui2/i18n/index.js";
import { mountI18nDemo } from "../src/ui2/i18n/demo.js";

test("MP-UI2-02: EN and ID catalogs have 100% key, placeholder, and product-name parity", () => {
  const parity = validateCatalogParity();
  assert.equal(parity.valid, true, JSON.stringify(parity));
  assert.deepEqual(parity.missingInId, []);
  assert.deepEqual(parity.extraInId, []);
  assert.deepEqual(parity.placeholderMismatches, []);
  assert.deepEqual(parity.productNameViolations, []);

  // Canonical glossary terms from UI-PRINCIPLES.md section 8
  assert.equal(t("en", "nav.home"), "Home");
  assert.equal(t("id", "nav.home"), "Beranda");
  assert.equal(t("en", "nav.memory"), "Memory");
  assert.equal(t("id", "nav.memory"), "Memori");
  assert.equal(t("en", "nav.constellation"), "Constellation");
  assert.equal(t("id", "nav.constellation"), "Konstelasi");
  assert.equal(t("en", "action.revoke"), "Revoke access");
  assert.equal(t("id", "action.revoke"), "Cabut akses");
  assert.equal(t("en", "action.viewOriginal"), "View original");
  assert.equal(t("id", "action.viewOriginal"), "Lihat asli");
});

test("MP-UI2-02: Intl.PluralRules, numbers, dates, and honest Unknown handling", () => {
  assert.equal(tPlural("en", "count.chats", 1), "1 chat");
  assert.equal(tPlural("en", "count.chats", 0), "0 chats");
  assert.equal(tPlural("en", "count.chats", 1240), "1,240 chats");

  assert.equal(tPlural("id", "count.chats", 1), "1 obrolan");
  assert.equal(tPlural("id", "count.chats", 1240), "1.240 obrolan");

  // Unknown must never render as 0 or Invalid Date
  assert.equal(tPlural("en", "count.chats", null), EN_CATALOG["state.unknown"]);
  assert.equal(tPlural("id", "count.chats", Number.NaN), ID_CATALOG["state.unknown"]);
  assert.equal(formatNumber("en", undefined), EN_CATALOG["state.unknown"]);
  assert.equal(formatNumber("id", Number.POSITIVE_INFINITY), ID_CATALOG["state.unknown"]);
  assert.equal(formatDate("en", "not-a-date"), EN_CATALOG["state.unknown"]);
  assert.equal(formatDate("id", null), ID_CATALOG["state.unknown"]);

  const sample = new Date(Date.UTC(2026, 10, 14, 10, 0, 0));
  const enDate = formatDate("en", sample, "medium");
  const idDate = formatDate("id", sample, "medium");
  assert.match(enDate, /2026/);
  assert.match(idDate, /2026/);
  assert.notEqual(enDate, idDate);
});

test("MP-UI2-02: Pseudo-locale expands prose while preserving placeholders and product names", () => {
  const sample = "Use Mind Place with ChatGPT ({count} chats) and SHA-256 Markdown.";
  const pseudo = toPseudoLocale(sample, { count: 12 });
  assert.ok(pseudo.length >= Math.ceil(sample.length * 1.3), `Expected >= 30% growth, got ${pseudo}`);
  for (const name of ["Mind Place", "ChatGPT", "SHA-256", "Markdown"]) {
    assert.ok(PROTECTED_PRODUCT_NAMES.includes(name));
    assert.ok(pseudo.includes(name), `Missing protected token ${name} in ${pseudo}`);
  }
  assert.ok(pseudo.includes("12"), `Interpolated count missing in ${pseudo}`);

  const templatePseudo = t("pseudo", "memory.translatedFrom", { lang: "English" });
  assert.ok(templatePseudo.includes("English"));
});

test("MP-UI2-02: Locale preference persists across restarts and respects View Original", () => {
  const store = new Map<string, string>();
  const storage = {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => {
      store.set(key, value);
    },
  };

  const firstSession = createI18nController({ storage });
  assert.equal(firstSession.getLocale(), "en");
  firstSession.setLocale("id");
  assert.equal(store.get(UI2_LOCALE_STORAGE_KEY), "id");

  // Simulate app restart with persisted storage
  const restartedSession = createI18nController({ storage });
  assert.equal(restartedSession.getLocale(), "id");

  const memoryInput = {
    statement: "Prefer strict vault isolation.",
    originalLocale: "en" as const,
    translations: {
      id: "Utamakan isolasi brankas yang ketat.",
    },
  };
  const translated = resolveCuratedMemoryCopy(memoryInput, "id", false);
  assert.equal(translated.isTranslated, true);
  assert.equal(translated.text, "Utamakan isolasi brankas yang ketat.");
  assert.equal(translated.translatedFromLabel, "Diterjemahkan dari Bahasa Inggris");
  assert.equal(translated.toggleActionLabel, "Lihat asli");

  const original = resolveCuratedMemoryCopy(memoryInput, "id", true);
  assert.equal(original.isTranslated, false);
  assert.equal(original.text, "Prefer strict vault isolation.");
  assert.equal(original.toggleActionLabel, "Lihat terjemahan");
});

test("MP-UI2-02: Demo mounts, switches language, and never translates user chat transcripts", () => {
  const dom = new JSDOM(`<!DOCTYPE html><html><body><div id="root"></div></body></html>`);
  const doc = dom.window.document;
  const root = doc.getElementById("root")!;
  const controller = createI18nController({ initialLocale: "en" });
  const handle = mountI18nDemo(root, { i18n: controller });

  const originalTranscript = doc.querySelector("[data-demo-transcript-body]")?.textContent;
  assert.equal(originalTranscript, formatUserChatText(EN_CATALOG["gallery.sample.userChatText"]));

  // Switch to Indonesian via button click
  doc.querySelector<HTMLButtonElement>('[data-demo-locale="id"]')?.click();
  assert.equal(doc.documentElement.lang, "id");
  assert.equal(doc.querySelector("[data-demo-translated-badge]")?.textContent, "Diterjemahkan dari Bahasa Inggris");
  assert.equal(doc.querySelector("[data-demo-memory-text]")?.textContent, ID_CATALOG["gallery.sample.memoryStatement"]);
  // User chat text remains verbatim English
  assert.equal(doc.querySelector("[data-demo-transcript-body]")?.textContent, originalTranscript);

  // Toggle "View original"
  doc.querySelector<HTMLButtonElement>("[data-demo-view-original]")?.click();
  assert.equal(doc.querySelector("[data-demo-memory-text]")?.textContent, EN_CATALOG["gallery.sample.memoryStatement"]);

  handle.destroy();
  assert.equal(root.childElementCount, 0);
});

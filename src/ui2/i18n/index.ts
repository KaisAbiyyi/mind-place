import { EN_CATALOG, type Ui2TranslationKey } from "./catalog-en.js";
import { ID_CATALOG } from "./catalog-id.js";

export { EN_CATALOG, type Ui2TranslationKey } from "./catalog-en.js";
export { ID_CATALOG } from "./catalog-id.js";

export type Ui2Locale = "en" | "id";
export type Ui2TestLocale = Ui2Locale | "pseudo";

export const UI2_LOCALES: readonly Ui2Locale[] = ["en", "id"] as const;
export const UI2_LOCALE_STORAGE_KEY = "mind_place.ui2.locale";

/**
 * Product, provider, and technical format names that must never be localized
 * in translations or pseudo-locale transformations.
 */
export const PROTECTED_PRODUCT_NAMES: readonly string[] = [
  "Mind Place",
  "Constellation",
  "ChatGPT",
  "Claude Desktop",
  "Claude",
  "Gemini",
  "MCP",
  "Chrome",
  "Edge",
  "Brave",
  "Markdown",
  "JSON",
  "HTML",
  "SHA-256",
  "SQLite",
] as const;

export type Ui2Params = Record<string, string | number | null | undefined>;

export interface Ui2StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export interface Ui2I18nController {
  getLocale(): Ui2TestLocale;
  setLocale(locale: Ui2TestLocale): void;
  t(key: Ui2TranslationKey, params?: Ui2Params): string;
  tPlural(baseKey: Ui2PluralBaseKey, count: number | null | undefined, params?: Ui2Params): string;
  formatDate(value: number | Date | string | null | undefined, style?: Ui2DateStyle): string;
  formatNumber(value: number | null | undefined): string;
  subscribe(listener: (locale: Ui2TestLocale) => void): () => void;
}

export type Ui2PluralBaseKey =
  | "count.chats"
  | "count.memories"
  | "count.sources"
  | "count.revisions"
  | "count.results"
  | "count.clients"
  | "count.connections";

export type Ui2DateStyle = "short" | "medium" | "datetime";

const PSEUDO_ACCENTS: Record<string, string> = {
  a: "\u00e5",
  b: "\u0180",
  c: "\u00e7",
  d: "\u00f0",
  e: "\u00e9",
  f: "\u0192",
  g: "\u011f",
  h: "\u0125",
  i: "\u00ef",
  j: "\u0135",
  k: "\u0137",
  l: "\u013c",
  m: "\u1e3f",
  n: "\u00f1",
  o: "\u00f6",
  p: "\u00fe",
  q: "\u01eb",
  r: "\u0155",
  s: "\u0161",
  t: "\u0165",
  u: "\u00fc",
  v: "\u1e7d",
  w: "\u0175",
  x: "\u1e8b",
  y: "\u00ff",
  z: "\u017e",
  A: "\u00c5",
  B: "\u0181",
  C: "\u00c7",
  D: "\u00d0",
  E: "\u00c9",
  F: "\u0191",
  G: "\u011e",
  H: "\u0124",
  I: "\u00cf",
  J: "\u0134",
  K: "\u0136",
  L: "\u013b",
  M: "\u1e3e",
  N: "\u00d1",
  O: "\u00d6",
  P: "\u00de",
  Q: "\u01ea",
  R: "\u0154",
  S: "\u0160",
  T: "\u0164",
  U: "\u00dc",
  V: "\u1e7c",
  W: "\u0174",
  X: "\u1e8a",
  Y: "\u0178",
  Z: "\u017d",
};

const CATALOGS: Record<Ui2Locale, Record<Ui2TranslationKey, string>> = {
  en: EN_CATALOG,
  id: ID_CATALOG,
};

function intlTagForLocale(locale: Ui2TestLocale): string {
  return locale === "id" ? "id-ID" : "en-US";
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

const PROTECTED_TOKEN_PATTERN = new RegExp(
  `(\\{[a-zA-Z0-9_]+\\}|${[...PROTECTED_PRODUCT_NAMES]
    .sort((a, b) => b.length - a.length)
    .map(escapeRegExp)
    .join("|")})`,
  "g"
);

function interpolate(template: string, params?: Ui2Params): string {
  if (!params) return template;
  return template.replace(/\{([a-zA-Z0-9_]+)\}/g, (match, name: string) => {
    const raw = params[name];
    if (raw === null || raw === undefined) return match;
    return String(raw);
  });
}

function resolveLocaleTagFromStorage(storage: Ui2StorageLike | undefined): Ui2Locale | null {
  if (!storage) return null;
  try {
    const raw = storage.getItem(UI2_LOCALE_STORAGE_KEY);
    return raw === "en" || raw === "id" ? raw : null;
  } catch {
    return null;
  }
}

function getDefaultStorage(): Ui2StorageLike | undefined {
  if (typeof globalThis !== "undefined" && "localStorage" in globalThis) {
    try {
      return globalThis.localStorage;
    } catch {
      return undefined;
    }
  }
  return undefined;
}

/**
 * Resolves a translation key strictly for `en`, `id`, or `pseudo` test mode.
 * Never falls back silently in strict test checks; runtime reads EN when an
 * unknown locale is requested.
 */
export function t(locale: Ui2TestLocale, key: Ui2TranslationKey, params?: Ui2Params): string {
  if (locale === "pseudo") {
    return toPseudoLocale(EN_CATALOG[key], params);
  }
  const catalog = CATALOGS[locale];
  const template = catalog[key];
  return interpolate(template, params);
}

/**
 * Formats counts with `Intl.PluralRules`. Non-finite or missing counts return
 * the localized Unknown label and are never coerced to `"0"`.
 */
export function tPlural(
  locale: Ui2TestLocale,
  baseKey: Ui2PluralBaseKey,
  count: number | null | undefined,
  extraParams?: Ui2Params
): string {
  if (count === null || count === undefined || typeof count !== "number" || !Number.isFinite(count)) {
    return t(locale, "state.unknown");
  }
  const effectiveLocale: Ui2Locale = locale === "pseudo" ? "en" : locale;
  const rule = new Intl.PluralRules(intlTagForLocale(effectiveLocale)).select(count);
  const candidateKey = `${baseKey}.${rule}` as Ui2TranslationKey;
  const fallbackKey = `${baseKey}.other` as Ui2TranslationKey;
  const key = candidateKey in EN_CATALOG ? candidateKey : fallbackKey;
  return t(locale, key, {
    ...extraParams,
    count: formatNumber(locale, count),
  });
}

/**
 * Formats numbers using `Intl.NumberFormat`. Missing or non-finite inputs
 * render as the localized `"Unknown"` string rather than `0`.
 */
export function formatNumber(locale: Ui2TestLocale, value: number | null | undefined): string {
  if (value === null || value === undefined || typeof value !== "number" || !Number.isFinite(value)) {
    return t(locale, "state.unknown");
  }
  return new Intl.NumberFormat(intlTagForLocale(locale)).format(value);
}

/**
 * Formats timestamps/dates via `Intl.DateTimeFormat`. Missing or invalid dates
 * return the localized `"Unknown"` string rather than `Invalid Date` or `0`.
 */
export function formatDate(
  locale: Ui2TestLocale,
  value: number | Date | string | null | undefined,
  style: Ui2DateStyle = "medium"
): string {
  if (value === null || value === undefined || value === "") {
    return t(locale, "state.unknown");
  }
  const date = value instanceof Date ? value : new Date(value);
  if (!Number.isFinite(date.getTime())) {
    return t(locale, "state.unknown");
  }
  const options: Intl.DateTimeFormatOptions =
    style === "short"
      ? { year: "2-digit", month: "short", day: "numeric" }
      : style === "datetime"
        ? { year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }
        : { year: "numeric", month: "short", day: "numeric" };
  return new Intl.DateTimeFormat(intlTagForLocale(locale), options).format(date);
}

/**
 * Produces ~35% expanded accented text for overflow and wrapping verification
 * while preserving `{placeholders}` and `PROTECTED_PRODUCT_NAMES` verbatim.
 */
export function toPseudoLocale(source: string, params?: Ui2Params): string {
  const segments = source.split(PROTECTED_TOKEN_PATTERN);
  let plainChars = "";
  const transformed = segments
    .map((segment) => {
      if (!segment) return "";
      if (
        /^\{[a-zA-Z0-9_]+\}$/.test(segment) ||
        PROTECTED_PRODUCT_NAMES.includes(segment)
      ) {
        return interpolate(segment, params);
      }
      plainChars += segment;
      let out = "";
      for (const ch of segment) {
        const mapped = PSEUDO_ACCENTS[ch] ?? ch;
        out += mapped;
        if ("aeiouAEIOU".includes(ch)) {
          out += mapped;
        }
      }
      return out;
    })
    .join("");

  const interpolated = interpolate(transformed, params);
  if (!plainChars.trim()) {
    return interpolated;
  }
  const targetDelta = Math.ceil(source.length * 0.35);
  const currentDelta = interpolated.length - source.length + 4;
  const suffixPadNeeded = Math.max(2, targetDelta - currentDelta);
  const tildeRun = "~".repeat(Math.min(24, suffixPadNeeded));
  return `[${interpolated} ${tildeRun}]`;
}

export interface CatalogParityResult {
  valid: boolean;
  missingInId: string[];
  extraInId: string[];
  placeholderMismatches: Array<{
    key: string;
    enPlaceholders: string[];
    idPlaceholders: string[];
  }>;
  productNameViolations: Array<{
    key: string;
    missingProductNames: string[];
  }>;
}

function extractPlaceholders(value: string): string[] {
  const matches = value.match(/\{[a-zA-Z0-9_]+\}/g) ?? [];
  return [...new Set(matches)].sort();
}

/**
 * Checks 100% key parity, placeholder parity, and untranslated product names
 * across `EN_CATALOG` and `ID_CATALOG`.
 */
export function validateCatalogParity(): CatalogParityResult {
  const enKeys = Object.keys(EN_CATALOG) as Ui2TranslationKey[];
  const idKeys = Object.keys(ID_CATALOG);
  const enSet = new Set<string>(enKeys);
  const idSet = new Set<string>(idKeys);

  const missingInId = enKeys.filter((k) => !idSet.has(k));
  const extraInId = idKeys.filter((k) => !enSet.has(k));
  const placeholderMismatches: CatalogParityResult["placeholderMismatches"] = [];
  const productNameViolations: CatalogParityResult["productNameViolations"] = [];

  for (const key of enKeys) {
    if (!idSet.has(key)) continue;
    const enValue = EN_CATALOG[key];
    const idValue = ID_CATALOG[key];
    const enPlaceholders = extractPlaceholders(enValue);
    const idPlaceholders = extractPlaceholders(idValue);
    if (enPlaceholders.join(",") !== idPlaceholders.join(",")) {
      placeholderMismatches.push({ key, enPlaceholders, idPlaceholders });
    }
    const missingNames = PROTECTED_PRODUCT_NAMES.filter(
      (name) => enValue.includes(name) && !idValue.includes(name)
    );
    if (missingNames.length > 0) {
      productNameViolations.push({ key, missingProductNames: missingNames });
    }
  }

  return {
    valid:
      missingInId.length === 0 &&
      extraInId.length === 0 &&
      placeholderMismatches.length === 0 &&
      productNameViolations.length === 0,
    missingInId,
    extraInId,
    placeholderMismatches,
    productNameViolations,
  };
}

export interface CuratedMemoryCopyInput {
  statement: string;
  originalLocale: Ui2Locale;
  translations?: Partial<Record<Ui2Locale, string>>;
}

export interface CuratedMemoryCopyResult {
  text: string;
  isTranslated: boolean;
  translatedFromLabel: string | null;
  toggleActionLabel: string | null;
}

/**
 * Resolves curated memory display copy according to app language (`N3`) and
 * `"View original"` toggle state. Never translates user conversation text.
 */
export function resolveCuratedMemoryCopy(
  input: CuratedMemoryCopyInput,
  appLocale: Ui2TestLocale,
  showOriginal = false
): CuratedMemoryCopyResult {
  const baseLocale: Ui2Locale = appLocale === "pseudo" ? "en" : appLocale;
  if (baseLocale === input.originalLocale) {
    return {
      text: appLocale === "pseudo" ? toPseudoLocale(input.statement) : input.statement,
      isTranslated: false,
      translatedFromLabel: null,
      toggleActionLabel: null,
    };
  }

  const translated = input.translations?.[baseLocale]?.trim();
  if (!translated) {
    return {
      text: input.statement,
      isTranslated: false,
      translatedFromLabel: null,
      toggleActionLabel: null,
    };
  }

  const originalLanguageName = t(appLocale, `langName.${input.originalLocale}` as Ui2TranslationKey);
  if (showOriginal) {
    return {
      text: input.statement,
      isTranslated: false,
      translatedFromLabel: t(appLocale, "memory.showingOriginal", { lang: originalLanguageName }),
      toggleActionLabel: t(appLocale, "action.viewTranslated"),
    };
  }

  return {
    text: appLocale === "pseudo" ? toPseudoLocale(translated) : translated,
    isTranslated: true,
    translatedFromLabel: t(appLocale, "memory.translatedFrom", { lang: originalLanguageName }),
    toggleActionLabel: t(appLocale, "action.viewOriginal"),
  };
}

/**
 * User and assistant chat messages always remain verbatim in their source language.
 */
export function formatUserChatText(rawMessage: string): string {
  return rawMessage;
}

export interface CreateI18nControllerOptions {
  initialLocale?: Ui2TestLocale;
  storage?: Ui2StorageLike;
}

export function createI18nController(options: CreateI18nControllerOptions = {}): Ui2I18nController {
  const storage = options.storage ?? getDefaultStorage();
  const stored = resolveLocaleTagFromStorage(storage);
  let current: Ui2TestLocale = options.initialLocale ?? stored ?? "en";
  const listeners = new Set<(locale: Ui2TestLocale) => void>();

  return {
    getLocale() {
      return current;
    },
    setLocale(locale) {
      if (current === locale) return;
      current = locale;
      if (storage && (locale === "en" || locale === "id")) {
        try {
          storage.setItem(UI2_LOCALE_STORAGE_KEY, locale);
        } catch {
          // Storage may be full or restricted in embedded test contexts.
        }
      }
      for (const listener of listeners) {
        listener(locale);
      }
    },
    t(key, params) {
      return t(current, key, params);
    },
    tPlural(baseKey, count, params) {
      return tPlural(current, baseKey, count, params);
    },
    formatDate(value, style) {
      return formatDate(current, value, style);
    },
    formatNumber(value) {
      return formatNumber(current, value);
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

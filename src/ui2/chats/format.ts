import type { Ui2I18nController } from "../i18n/index.js";

/** Product names are never translated. Unknown providers get a readable capitalized name. */
export function providerName(provider: string): string {
  const key = provider.trim().toLowerCase();
  if (key === "chatgpt") return "ChatGPT";
  if (key === "claude") return "Claude";
  if (key === "gemini") return "Gemini";
  return key ? key[0]!.toUpperCase() + key.slice(1) : "";
}

export function providerKey(provider: string): string {
  return provider.trim().toLowerCase();
}

/** "1 message" / "2 messages" with the app language's plural rules. */
export function messagesLabel(i18n: Ui2I18nController, count: number): string {
  const locale = i18n.getLocale() === "id" ? "id-ID" : "en-US";
  const rule = new Intl.PluralRules(locale).select(count);
  return i18n.t(rule === "one" ? "arch.msgs.one" : "arch.msgs.other", { n: i18n.formatNumber(count) });
}

/** Only http and https pages are linked; anything else is not shown as a link. */
export function safeHttpHost(url: string): { href: string; host: string } | null {
  try {
    const parsed = new URL(url);
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return null;
    return { href: parsed.href, host: parsed.hostname };
  } catch {
    return null;
  }
}

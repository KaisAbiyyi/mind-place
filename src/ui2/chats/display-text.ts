import type { Message } from "../../contracts.js";

/**
 * Display-layer only. Stored captures are raw evidence and are never changed.
 * A provider's own page wraps an answer in labels the person never wrote:
 * a leading "Claude responded:" and a trailing "just now" clock. They are not
 * part of the conversation, so the reader does not show them.
 *
 * Deliberately narrow so real words are never removed:
 * - only assistant messages are touched, never the owner's own words;
 * - the leading label must be the conversation's own provider label at the
 *   very start of the message;
 * - a clock is removed only when it is the whole last line;
 * - if nothing would be left, the original text is kept.
 */
const LEADING_LABEL: Readonly<Record<string, RegExp>> = {
  claude: /^\s*Claude responded[ \t]*:[ \t]*\n?/,
  chatgpt: /^\s*ChatGPT said[ \t]*:[ \t]*\n?/,
  gemini: /^\s*Gemini said[ \t]*:[ \t]*\n?/,
};

const TRAILING_CLOCK =
  /\n[ \t]*(?:just now|(?:a|an|\d+)[ \t]+(?:seconds?|minutes?|hours?|days?|weeks?|months?|years?)[ \t]+ago)[ \t]*$/i;

export function displayMessageText(message: Pick<Message, "role" | "text">, provider: string): string {
  if (message.role !== "assistant") return message.text;
  let text = message.text;
  const key = provider.toLowerCase();
  const label = Object.keys(LEADING_LABEL).find((name) => key.startsWith(name));
  if (label) {
    const stripped = text.replace(LEADING_LABEL[label]!, "");
    if (stripped.trim()) text = stripped;
  }
  const withoutClock = text.trimEnd().replace(TRAILING_CLOCK, "");
  if (withoutClock.trim()) text = withoutClock;
  return text;
}

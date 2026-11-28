import assert from "node:assert/strict";
import test from "node:test";
import { isSafeImageDataUrl, ALLOWED_IMAGE_MIME_TYPES } from "../src/ui2/chats/assets.js";
import { displayMessageText } from "../src/ui2/chats/display-text.js";

const assistant = (text: string) => ({ role: "assistant" as const, text });

test("display text removes a provider's leading label and trailing clock from assistant answers only", () => {
  assert.equal(displayMessageText(assistant("Claude responded: Hello there"), "claude"), "Hello there");
  assert.equal(displayMessageText(assistant("Claude responded:\nHello there"), "claude"), "Hello there");
  assert.equal(displayMessageText(assistant("ChatGPT said:\nHi"), "chatgpt"), "Hi");
  assert.equal(displayMessageText(assistant("Gemini said: Hi"), "gemini"), "Hi");
  assert.equal(displayMessageText(assistant("Answer text\njust now"), "claude"), "Answer text");
  assert.equal(displayMessageText(assistant("Answer text\n5 minutes ago"), "claude"), "Answer text");
  assert.equal(displayMessageText(assistant("Answer text\nan hour ago  "), "chatgpt"), "Answer text");
  assert.equal(displayMessageText(assistant("Claude responded: Both\nthere\n2 days ago"), "claude"), "Both\nthere");
});

test("display text never edits the owner's words or real sentences", () => {
  const user = { role: "user" as const, text: "Claude responded: is this a label?\njust now" };
  assert.equal(displayMessageText(user, "claude"), user.text);
  // A label from another provider is not this conversation's chrome.
  assert.equal(displayMessageText(assistant("Claude responded: x"), "chatgpt"), "Claude responded: x");
  // Mid-sentence words stay.
  assert.equal(displayMessageText(assistant("It ended just now, honestly."), "claude"), "It ended just now, honestly.");
  assert.equal(displayMessageText(assistant("Added 3 days ago in the notes"), "claude"), "Added 3 days ago in the notes");
  // A label only in the middle stays.
  assert.equal(displayMessageText(assistant("Intro\nClaude responded: later"), "claude"), "Intro\nClaude responded: later");
  // Nothing left would mean losing the whole message: keep it.
  assert.equal(displayMessageText(assistant("Claude responded:"), "claude"), "Claude responded:");
  assert.equal(displayMessageText(assistant("just now"), "claude"), "just now");
  // Tool and system text is untouched.
  assert.equal(displayMessageText({ role: "tool", text: "x\njust now" }, "claude"), "x\njust now");
});

test("picture validation: raster types only, exact data-URL prefix, strict base64, size cap", () => {
  assert.deepEqual([...ALLOWED_IMAGE_MIME_TYPES].sort(), ["image/avif", "image/gif", "image/jpeg", "image/png", "image/webp"]);
  assert.equal(isSafeImageDataUrl("data:image/png;base64,iVBORw0KGgo=", "image/png"), true);
  assert.equal(isSafeImageDataUrl("data:image/jpeg;base64,iVBORw0KGgo=", "image/png"), false, "prefix must match the declared type");
  assert.equal(isSafeImageDataUrl("data:image/png;base64,AAAA<script>", "image/png"), false);
  assert.equal(isSafeImageDataUrl("https://evil.example/p.png", "image/png"), false);
  assert.equal(isSafeImageDataUrl("data:image/png;base64,AA=A", "image/png"), false);
  assert.equal(isSafeImageDataUrl("data:image/png;base64," + "A".repeat(33 * 1024 * 1024), "image/png"), false);
});

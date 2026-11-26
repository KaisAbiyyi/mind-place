import assert from "node:assert/strict";
import test from "node:test";
import { JSDOM } from "jsdom";
import type { Capture, CaptureSnapshot, Conversation, ConversationSummary, Message } from "../src/contracts.js";
import type { Invoke } from "../src/reader.js";
import { createArchivePage } from "../src/ui2/chats/page.js";
import { createI18nController } from "../src/ui2/i18n/index.js";
import { mountUi2Shell } from "../src/ui2/shell/shell.js";
import { createUi2Pages } from "../src/ui2/shell/pages.js";
import type { Ui2PageDetail } from "../src/ui2/shell/page.js";

const sid = (n: number) => `src:v1:${n.toString(16).padStart(64, "0")}`;
const snap = (n: number) => n.toString(16).padStart(64, "0");
const PNG = "data:image/png;base64,iVBORw0KGgo=";

function summary(n: number, over: Partial<ConversationSummary> = {}): ConversationSummary {
  return {
    id: sid(n),
    conversation_id: `conv-${n}`,
    provider: "chatgpt",
    title: `Chat number ${n}`,
    source_url: "https://chatgpt.com/c/synthetic",
    conversation_date: "2026-11-01T10:00:00Z",
    date_source: "provider-updated",
    captured_at: `2026-11-0${(n % 9) + 1}T10:00:00Z`,
    message_count: 2,
    snapshot_count: 1,
    completeness: "complete",
    warnings: [],
    ...over,
  };
}

function msg(i: number, role: Message["role"], text: string, over: Partial<Message> = {}): Message {
  return { id: `m${i}`, role, text, format: "markdown", attachments: [], ...over };
}

function conversation(s: ConversationSummary, messages: Message[], over: Partial<Capture> = {}, snapshotId?: string): Conversation {
  const capture: Capture = {
    schema_version: 1,
    provider: "chatgpt",
    conversation_id: s.conversation_id,
    source_url: s.source_url,
    title: s.title,
    captured_at: s.captured_at,
    capture_method: "manual-dom",
    completeness: s.completeness,
    warnings: [],
    messages,
    ...over,
  };
  return { summary: s, capture, ...(snapshotId ? { snapshot_id: snapshotId } : {}) };
}

interface RigOptions {
  summaries?: ConversationSummary[];
  conversations?: Record<string, Conversation>;
  /** Extra copies by archive id, keyed by snapshot id. */
  copies?: Record<string, Record<string, Conversation>>;
  snapshots?: Record<string, CaptureSnapshot[]>;
  assets?: Record<string, { mime_type: string; data_url: string }>;
  failList?: boolean;
  failRead?: boolean;
  failCopy?: string;
  exportResult?: { path: string; markdown: string };
  failExport?: boolean;
  listGate?: Promise<void>;
  readGate?: Promise<void>;
  clipboard?: "ok" | "reject" | "missing";
  withShellHooks?: boolean;
}

function rig(options: RigOptions = {}) {
  const dom = new JSDOM('<!doctype html><body><div id="root"></div></body>', { pretendToBeVisual: true });
  const doc = dom.window.document;
  Object.assign(globalThis, { document: doc, window: dom.window });
  const calls: Array<{ command: string; args: Record<string, unknown> }> = [];
  const summaries = options.summaries ?? [];
  let failList = options.failList ?? false;
  const invoke: Invoke = async <T>(command: string, args: Record<string, unknown> = {}) => {
    calls.push({ command, args });
    switch (command) {
      case "list_conversations":
        await options.listGate;
        if (failList) throw new Error("MP_X");
        return summaries.map((s) => ({ ...s })) as T;
      case "read_conversation": {
        await options.readGate;
        if (options.failRead) throw new Error("MP_X");
        const id = String(args.id);
        if (args.snapshotId) {
          if (options.failCopy === args.snapshotId) throw new Error("MP_X");
          const found = options.copies?.[id]?.[String(args.snapshotId)];
          if (!found) throw new Error("MP_NOT_FOUND");
          return found as T;
        }
        const found = options.conversations?.[id];
        if (!found) throw new Error("MP_NOT_FOUND");
        return found as T;
      }
      case "list_snapshots":
        return (options.snapshots?.[String(args.id)] ?? []) as T;
      case "get_asset": {
        const asset = options.assets?.[String(args.assetPath)];
        if (!asset) throw new Error("MP_NO_ASSET");
        return asset as T;
      }
      case "export_markdown":
        if (options.failExport) throw new Error("MP_X");
        return (options.exportResult ?? { path: "", markdown: "# Synthetic export" }) as T;
      default:
        throw new Error(`unexpected ${command}`);
    }
  };
  const i18n = createI18nController({ initialLocale: "en", storage: { getItem: () => null, setItem: () => {} } });
  const written: string[] = [];
  const details: Array<Ui2PageDetail | null> = [];
  const navigated: string[] = [];
  const page = createArchivePage({
    doc,
    i18n,
    invoke,
    ...(options.clipboard === "missing"
      ? {}
      : {
          clipboard: {
            writeText: async (value: string) => {
              if (options.clipboard === "reject") throw new Error("NotAllowedError");
              written.push(value);
            },
          },
        }),
    ...(options.withShellHooks === false
      ? {}
      : { setDetail: (d: Ui2PageDetail | null) => { details.push(d); }, navigate: (p: string) => { navigated.push(p); } }),
  });
  doc.getElementById("root")!.append(page.element);
  const settle = async () => {
    for (let i = 0; i < 10; i += 1) await new Promise((resolve) => setTimeout(resolve, 0));
  };
  const buttons = () => [...page.element.querySelectorAll<HTMLButtonElement>("button")];
  const button = (label: string) => buttons().find((b) => b.textContent === label);
  const rows = () => [...page.element.querySelectorAll<HTMLButtonElement>("button.ui2-list-row")];
  const key = (target: Element, name: string, shiftKey = false) =>
    target.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: name, shiftKey, bubbles: true, cancelable: true }));
  return { dom, doc, page, calls, i18n, settle, button, buttons, rows, key, written, details, navigated, setFailList: (v: boolean) => { failList = v; } };
}

function oneChat(over: Partial<ConversationSummary> = {}, messages?: Message[], captureOver: Partial<Capture> = {}) {
  const s = summary(1, { title: "Mind-place save testing", provider: "claude", ...over });
  const msgs = messages ?? [
    msg(1, "user", "this chat is for mind-place save testing"),
    msg(2, "assistant", "Claude responded: Got it, this chat is set aside for your testing.\n\nWhat would you like to try first?\njust now"),
  ];
  return { s, conv: conversation(s, msgs, captureOver, snap(11)) };
}

async function openFirst(r: ReturnType<typeof rig>) {
  await r.settle();
  r.rows()[0]!.click();
  await r.settle();
}

test("lists saved chats with provider chips, one calm row each, and no identifiers", async () => {
  const r = rig({
    summaries: [
      summary(1, { provider: "chatgpt", title: "Capture host: Rust or Go?" }),
      summary(2, { provider: "claude", title: "Mind-place save testing", completeness: "partial" }),
      summary(3, { provider: "chatgpt", title: "K-Means notes" }),
    ],
  });
  await r.settle();
  const text = r.page.element.textContent ?? "";
  assert.match(text, /Everything you saved or imported, exactly as it was/);
  const chips = [...r.page.element.querySelectorAll<HTMLButtonElement>(".ui2-chip")].map((c) => c.textContent);
  assert.deepEqual(chips, ["All", "ChatGPT", "Claude", "Gemini"]);
  assert.equal(r.rows().length, 3);
  assert.match(r.rows()[1]!.textContent ?? "", /Mind-place save testing.*Claude · saved .*Partial/s);
  assert.equal(r.page.element.querySelectorAll(".ui2-tag").length, 1, "only the partial chat carries a tag");
  assert.doesNotMatch(r.doc.body.innerHTML, /src:v1:|sha256|rev_|conv-/);
  assert.match(text, /Showing 1 to 3 of 3/);

  // Provider filter.
  r.page.element.querySelector<HTMLButtonElement>('[data-ui2-chip="claude"]')!.click();
  assert.equal(r.rows().length, 1);
  assert.equal(r.page.element.querySelector('[data-ui2-chip="claude"]')?.getAttribute("aria-pressed"), "true");
  r.page.element.querySelector<HTMLButtonElement>('[data-ui2-chip="gemini"]')!.click();
  assert.equal(r.rows().length, 0);
  assert.match(r.page.element.textContent ?? "", /No chats from Gemini yet/);
  r.page.destroy();
});

test("a provider found only in the archive gets its own chip", async () => {
  const r = rig({ summaries: [summary(1, { provider: "perplexity" })] });
  await r.settle();
  const chips = [...r.page.element.querySelectorAll<HTMLButtonElement>(".ui2-chip")].map((c) => c.textContent);
  assert.deepEqual(chips, ["All", "ChatGPT", "Claude", "Gemini", "Perplexity"]);
  r.page.destroy();
});

test("Import is shown but unavailable, says why, and never calls an import command", async () => {
  const r = rig({ summaries: [summary(1)] });
  await r.settle();
  const importButton = r.button("Import")!;
  assert.ok(importButton, "the drawn Import button is present");
  assert.equal(importButton.disabled, true);
  const hintId = importButton.getAttribute("aria-describedby")!;
  const hint = r.doc.getElementById(hintId)!;
  assert.match(hint.textContent ?? "", /not available in this version yet/);
  assert.match(hint.textContent ?? "", /ChatGPT or Gemini export/);
  importButton.click();
  await r.settle();
  assert.equal(r.calls.filter((c) => /import|selection|inspect/.test(c.command)).length, 0);
  r.page.destroy();
});

test("a long archive is paged at 50 rows and keeps keyboard focus on the pager", async () => {
  const many = Array.from({ length: 65 }, (_, i) => summary(i + 1));
  const r = rig({ summaries: many });
  await r.settle();
  assert.equal(r.rows().length, 50);
  assert.match(r.page.element.textContent ?? "", /Showing 1 to 50 of 65/);
  const next = r.button("Next")!;
  next.focus();
  next.click();
  assert.equal(r.rows().length, 15);
  assert.match(r.page.element.textContent ?? "", /Showing 51 to 65 of 65/);
  assert.equal(r.doc.activeElement, r.button("Previous"), "the used button ended its run, focus moves to its partner");
  r.button("Previous")!.click();
  assert.equal(r.rows().length, 50);
  r.page.destroy();
});

test("empty, loading and error states are plain and offer one way forward", async () => {
  const gate = (() => { let release!: () => void; const promise = new Promise<void>((resolve) => { release = resolve; }); return { promise, release }; })();
  const loading = rig({ summaries: [], listGate: gate.promise });
  assert.ok(loading.page.element.querySelector('[role="status"][aria-busy="true"]'), "skeleton while loading");
  gate.release();
  await loading.settle();
  assert.match(loading.page.element.textContent ?? "", /No saved chats yet/);
  loading.button("Set up saving")!.click();
  assert.deepEqual(loading.navigated, ["/connections"]);
  loading.page.destroy();

  const noShell = rig({ summaries: [], withShellHooks: false });
  await noShell.settle();
  assert.equal(noShell.button("Set up saving"), undefined, "no action without a shell to go to");
  noShell.page.destroy();

  const failing = rig({ summaries: [summary(1)], failList: true });
  await failing.settle();
  assert.match(failing.page.element.textContent ?? "", /Could not load your saved chats/);
  failing.setFailList(false);
  failing.button("Retry")!.click();
  await failing.settle();
  assert.equal(failing.rows().length, 1);
  failing.page.destroy();
});

test("the reader is a chat: owner on the right, assistant on the left, provider labels hidden, evidence untouched", async () => {
  const { s, conv } = oneChat();
  const r = rig({ summaries: [s], conversations: { [s.id]: conv }, snapshots: { [s.id]: [{ snapshot_id: snap(11), captured_at: s.captured_at, message_count: 2, completeness: "complete" }] } });
  await openFirst(r);
  const reader = r.page.element.querySelector(".ui2-chats-reader")!;
  assert.ok(reader, "reader mounted");
  assert.equal(reader.querySelector("h1")?.textContent, "Mind-place save testing");
  assert.equal(reader.querySelectorAll(".ui2-chats-user").length, 1);
  assert.equal(reader.querySelectorAll(".ui2-chats-ai").length, 1);
  assert.match(reader.querySelector(".ui2-chats-user")?.textContent ?? "", /this chat is for mind-place save testing/);
  assert.equal(reader.querySelector(".ui2-chats-avatar")?.textContent, "C");
  const aiText = reader.querySelector(".ui2-chats-ai")?.textContent ?? "";
  assert.match(aiText, /Got it, this chat is set aside/);
  assert.doesNotMatch(aiText, /Claude responded/);
  assert.doesNotMatch(aiText, /just now/);
  // Raw evidence is exactly as saved.
  assert.match(conv.capture.messages[1]!.text, /^Claude responded: Got it/);
  assert.match(conv.capture.messages[1]!.text, /just now$/);
  // The shell was told the title; the list is hidden but kept.
  assert.equal(r.details.at(-1)?.title, "Mind-place save testing");
  assert.equal(r.page.element.querySelector<HTMLElement>(".ui2-stack[hidden]") !== null, true);
  // Default view carries no ids: they live only inside Technical details.
  const leaves = [...r.doc.querySelectorAll("*")].filter((e) => e.children.length === 0 && /src:v1:|[0-9a-f]{64}/.test(e.textContent ?? ""));
  assert.ok(leaves.length > 0, "the ids exist somewhere");
  assert.ok(leaves.every((e) => e.closest("[data-ui2-technical-details]")), "only inside Technical details");
  r.page.destroy();
});

test("the reader uses the shared sanitizer: scripts, handlers, remote pictures and bad links are inert", async () => {
  const evil = [
    "<script>window.__pwn=1</script>",
    "<img src=x onerror=\"window.__pwn=2\">",
    "![remote](https://evil.example/p.png)",
    "[bad](javascript:alert(1))",
    "[good](https://example.com/page)",
  ].join("\n\n");
  const { s, conv } = oneChat({}, [msg(1, "user", "hi"), msg(2, "assistant", evil)]);
  const r = rig({ summaries: [s], conversations: { [s.id]: conv } });
  await openFirst(r);
  const html = r.page.element.querySelector(".ui2-chats-ai")!.innerHTML;
  assert.doesNotMatch(html, /<script|onerror|evil\.example|javascript:/i);
  assert.equal(r.page.element.querySelectorAll(".ui2-chats-ai img").length, 0);
  const good = r.page.element.querySelector<HTMLAnchorElement>('.ui2-chats-ai a[href^="https://example.com"]')!;
  assert.equal(good.rel, "noopener noreferrer");
  assert.equal((globalThis as { __pwn?: number }).__pwn, undefined);
  r.page.destroy();
});

test("pictures load through get_asset with the classic validation; files and failures are said in words", async () => {
  const s = summary(1);
  const conv = conversation(s, [
    msg(1, "user", "see these"),
    msg(2, "assistant", "ok", {
      attachments: [
        { id: "a1", name: "Diagram", mime_type: "image/png", asset_path: "assets/one.png" },
        { id: "a2", name: "Vector", mime_type: "image/svg+xml", asset_path: "assets/two.svg" },
        { id: "a3", name: "Spoof", mime_type: "image/png", asset_path: "assets/three.png" },
        { id: "a4", name: "Gone", mime_type: "image/png" },
        { id: "a5", name: "notes.pdf", mime_type: "application/pdf" },
      ],
    }),
  ]);
  const r = rig({
    summaries: [s],
    conversations: { [s.id]: conv },
    assets: {
      "assets/one.png": { mime_type: "image/png", data_url: PNG },
      "assets/two.svg": { mime_type: "image/svg+xml", data_url: "data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=" },
      "assets/three.png": { mime_type: "image/png", data_url: "data:image/png;base64,AAAA<script>" },
    },
  });
  await openFirst(r);
  await r.settle();
  const images = [...r.page.element.querySelectorAll<HTMLImageElement>("img.ui2-chats-image")];
  assert.equal(images.length, 1, "only the valid PNG becomes an image");
  assert.equal(images[0]!.src, PNG);
  assert.equal(images[0]!.alt, "Diagram");
  const text = r.page.element.textContent ?? "";
  assert.equal((text.match(/This picture could not be shown/g) ?? []).length, 2, "svg and spoofed data are refused");
  assert.match(text, /This picture is not available in your archive/);
  assert.match(text, /notes\.pdf/);
  assert.deepEqual(
    r.calls.filter((c) => c.command === "get_asset").map((c) => c.args),
    [{ id: s.id, assetPath: "assets/one.png" }, { id: s.id, assetPath: "assets/two.svg" }, { id: s.id, assetPath: "assets/three.png" }],
  );
  r.page.destroy();
});

test("a long transcript shows one page of 40 messages at a time", async () => {
  const s = summary(1);
  const messages = Array.from({ length: 100 }, (_, i) => msg(i, i % 2 ? "assistant" : "user", `message body ${i + 1}`));
  const r = rig({ summaries: [s], conversations: { [s.id]: conversation(s, messages) } });
  await openFirst(r);
  const count = () => r.page.element.querySelectorAll(".ui2-chats-msg").length;
  assert.equal(count(), 40);
  assert.match(r.page.element.textContent ?? "", /Messages 1 to 40 of 100/);
  assert.equal(r.button("Earlier messages")!.disabled, true);
  r.button("Later messages")!.click();
  assert.equal(count(), 40);
  assert.match(r.page.element.textContent ?? "", /Messages 41 to 80 of 100/);
  assert.match(r.page.element.textContent ?? "", /message body 41/);
  assert.doesNotMatch(r.page.element.textContent ?? "", /message body 40\b/);
  assert.equal(r.doc.activeElement, r.page.element.querySelector(".ui2-chats-msg"), "focus moves to the first message of the new page");
  r.button("Later messages")!.click();
  assert.equal(count(), 20);
  assert.equal(r.button("Later messages")!.disabled, true);
  r.page.destroy();
});

test("tool and system steps stay left, quiet and labelled", async () => {
  const s = summary(1);
  const r = rig({ summaries: [s], conversations: { [s.id]: conversation(s, [msg(1, "system", "Be brief."), msg(2, "tool", "search finished"), msg(3, "user", "hello")]) } });
  await openFirst(r);
  const aux = [...r.page.element.querySelectorAll(".ui2-chats-aux")];
  assert.equal(aux.length, 2);
  assert.deepEqual(aux.map((a) => a.querySelector(".ui2-chats-role")?.textContent), ["System", "Tool"]);
  r.page.destroy();
});

test("Info aside: keyboard, focus trap, Escape and focus return", async () => {
  const { s, conv } = oneChat({ completeness: "complete" });
  const r = rig({ summaries: [s], conversations: { [s.id]: conv }, snapshots: { [s.id]: [{ snapshot_id: snap(11), captured_at: s.captured_at, message_count: 2, completeness: "complete" }] } });
  await openFirst(r);
  const aside = r.page.element.querySelector<HTMLElement>(".ui2-chats-aside")!;
  assert.equal(aside.hidden, true, "Info starts closed");
  const info = r.button("Info")!;
  info.focus();
  info.click();
  assert.equal(aside.hidden, false);
  assert.ok(aside.contains(r.doc.activeElement), "focus moves into the aside");
  assert.equal(aside.getAttribute("aria-modal"), "false");
  const title = aside.querySelector("h3")?.textContent;
  assert.equal(title, "About this chat");

  const text = aside.textContent ?? "";
  for (const group of ["Source", "Saved copies", "Completeness", "Technical details"]) assert.match(text, new RegExp(group, "i"));
  assert.match(text, /Saved by you, from the browser/);
  assert.match(text, /chatgpt\.com/);
  const tech = aside.querySelector<HTMLDetailsElement>("[data-ui2-technical-details]")!;
  assert.equal(tech.open, false, "Technical details start collapsed");

  // Tab wraps inside the aside both ways.
  const focusables = [...aside.querySelectorAll<HTMLElement>("button:not([disabled]), summary")];
  focusables.at(-1)!.focus();
  const wrapped = r.key(focusables.at(-1)!, "Tab");
  assert.equal(wrapped, false, "Tab at the end is handled by the trap");
  assert.equal(r.doc.activeElement, focusables[0]);
  r.key(focusables[0]!, "Tab", true);
  assert.equal(r.doc.activeElement, focusables.at(-1));

  // Escape closes and returns focus to Info.
  r.key(r.doc.activeElement!, "Escape");
  assert.equal(aside.hidden, true);
  assert.equal(r.doc.activeElement, info);

  // The close button works with the keyboard click too.
  info.click();
  aside.querySelector<HTMLButtonElement>("button")!.click();
  assert.equal(aside.hidden, true);
  assert.equal(r.doc.activeElement, info);
  r.page.destroy();
});

test("a partial copy: one banner sentence, and Why? opens Info on Completeness then returns focus", async () => {
  const { s, conv } = oneChat({ completeness: "partial" }, undefined, { completeness: "partial", warnings: ["dom-not-fully-loaded"] });
  const r = rig({ summaries: [s], conversations: { [s.id]: conv } });
  await openFirst(r);
  const banners = r.page.element.querySelectorAll(".ui2-banner");
  assert.equal(banners.length, 1, "one banner per view");
  assert.match(banners[0]!.textContent ?? "", /This copy may be missing some messages\./);
  const why = r.button("Why?")!;
  why.focus();
  why.click();
  const aside = r.page.element.querySelector<HTMLElement>(".ui2-chats-aside")!;
  assert.equal(aside.hidden, false);
  assert.equal(r.doc.activeElement?.textContent, "Completeness");
  assert.match(aside.textContent ?? "", /Mind Place never scrolls on its own/);
  // Notes written while saving are only inside Technical details.
  const note = [...aside.querySelectorAll("code")].find((c) => /dom-not-fully-loaded/.test(c.textContent ?? ""))!;
  assert.ok(note.closest("[data-ui2-technical-details]"));
  r.key(r.doc.activeElement!, "Escape");
  assert.equal(aside.hidden, true);
  assert.equal(r.doc.activeElement, why);
  r.page.destroy();
});

test("a complete copy has no banner and says nothing was noticed missing", async () => {
  const { s, conv } = oneChat();
  const r = rig({ summaries: [s], conversations: { [s.id]: conv } });
  await openFirst(r);
  assert.equal(r.page.element.querySelectorAll(".ui2-banner").length, 0);
  assert.equal(r.button("Why?"), undefined);
  r.button("Info")!.click();
  assert.match(r.page.element.querySelector(".ui2-chats-aside")?.textContent ?? "", /No missing messages were noticed/);
  r.page.destroy();
});

test("saved copies are switched inside Info; a failed switch changes nothing", async () => {
  const s = summary(1, { snapshot_count: 3 });
  const latest = conversation(s, [msg(1, "user", "latest question"), msg(2, "assistant", "latest answer")], {}, snap(3));
  const middle = conversation(s, [msg(1, "user", "older question")], {}, snap(2));
  const oldest = conversation(s, [msg(1, "user", "oldest question")], {}, snap(1));
  const r = rig({
    summaries: [s],
    conversations: { [s.id]: latest },
    copies: { [s.id]: { [snap(2)]: middle, [snap(1)]: oldest } },
    failCopy: snap(1),
    snapshots: {
      [s.id]: [
        { snapshot_id: snap(3), captured_at: "2026-11-03T10:00:00Z", message_count: 2, completeness: "complete" },
        { snapshot_id: snap(2), captured_at: "2026-11-02T10:00:00Z", message_count: 1, completeness: "complete" },
        { snapshot_id: snap(1), captured_at: "2026-11-01T10:00:00Z", message_count: 1, completeness: "partial" },
      ],
    },
  });
  await openFirst(r);
  r.button("Info")!.click();
  const aside = r.page.element.querySelector<HTMLElement>(".ui2-chats-aside")!;
  const items = [...aside.querySelectorAll(".ui2-chats-copy")];
  assert.equal(items.length, 3);
  assert.match(items[0]!.textContent ?? "", /Latest copy.*2 messages.*Showing/s);
  assert.match(items[1]!.textContent ?? "", /Earlier copy.*1 message(?!s)/s);
  const idLeaves = [...aside.querySelectorAll("*")].filter((e) => e.children.length === 0 && /src:v1:|[0-9a-f]{64}/.test(e.textContent ?? ""));
  assert.ok(idLeaves.every((e) => e.closest("[data-ui2-technical-details]")), "copy ids appear only inside Technical details");

  // Switch to the middle copy.
  [...aside.querySelectorAll<HTMLButtonElement>("button")].filter((b) => b.textContent === "View this copy")[0]!.click();
  await r.settle();
  assert.match(r.page.element.querySelector(".ui2-chats-thread")?.textContent ?? "", /older question/);
  assert.doesNotMatch(r.page.element.querySelector(".ui2-chats-thread")?.textContent ?? "", /latest question/);
  assert.deepEqual(r.calls.filter((c) => c.command === "read_conversation").at(-1)!.args, { id: s.id, snapshotId: snap(2) });
  const active = aside.querySelector<HTMLElement>("[data-ui2-copy-active]")!;
  assert.match(active.textContent ?? "", /Earlier copy.*Showing/s);
  assert.equal(r.doc.activeElement, active, "focus follows to the copy now showing");

  // A copy that fails to open leaves everything as it was and says so.
  [...aside.querySelectorAll<HTMLButtonElement>("button")].filter((b) => b.textContent === "View this copy").at(-1)!.click();
  await r.settle();
  assert.match(aside.querySelector('[role="alert"]')?.textContent ?? "", /Could not open that copy\. Nothing was changed\./);
  assert.match(r.page.element.querySelector(".ui2-chats-thread")?.textContent ?? "", /older question/);
  r.page.destroy();
});

test("Copy copies the same words the reader shows and says Copied; a failed copy says so", async () => {
  const { s, conv } = oneChat();
  const ok = rig({ summaries: [s], conversations: { [s.id]: conv } });
  await openFirst(ok);
  ok.button("Copy")!.click();
  await ok.settle();
  assert.equal(ok.written.length, 1);
  assert.match(ok.written[0]!, /^You\nthis chat is for mind-place save testing\n\nAssistant\nGot it/);
  assert.doesNotMatch(ok.written[0]!, /Claude responded|just now/);
  assert.ok(ok.button("Copied"), "label confirms the copy");
  ok.page.destroy();

  const bad = rig({ summaries: [s], conversations: { [s.id]: conv }, clipboard: "reject" });
  await openFirst(bad);
  bad.button("Copy")!.click();
  await bad.settle();
  assert.match(bad.page.element.textContent ?? "", /Could not copy\. Select the text and press Ctrl\+C\./);
  assert.equal(bad.button("Copied"), undefined, "no false Copied");
  bad.page.destroy();
});

test("Export: Markdown only; a saved file shows where, an in-memory result offers Copy and Save as", async () => {
  const { s, conv } = oneChat();
  const withFile = rig({ summaries: [s], conversations: { [s.id]: conv }, exportResult: { path: "C:\\Users\\owner\\Exports\\chat.md", markdown: "# One" } });
  await openFirst(withFile);
  const exportButton = withFile.button("Export")!;
  exportButton.focus();
  exportButton.click();
  const aside = [...withFile.page.element.querySelectorAll<HTMLElement>(".ui2-chats-aside")].find((a) => !a.hidden)!;
  assert.equal(aside.querySelector("h3")?.textContent, "Export");
  const markdownButton = [...aside.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Markdown")!;
  markdownButton.click();
  await withFile.settle();
  assert.match(aside.textContent ?? "", /Saved as a file\. Find it here: C:\\Users\\owner\\Exports\\chat\.md/);
  assert.equal([...aside.querySelectorAll("button")].some((b) => b.textContent === "Save as..."), false);
  [...aside.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Copy Markdown")!.click();
  await withFile.settle();
  assert.deepEqual(withFile.written, ["# One"]);
  assert.match(aside.textContent ?? "", /Markdown copied\./);
  assert.deepEqual(withFile.calls.filter((c) => c.command === "export_markdown").map((c) => c.args), [{ id: s.id, snapshotId: snap(11) }]);
  withFile.key(withFile.doc.activeElement!, "Escape");
  assert.equal(aside.hidden, true);
  assert.equal(withFile.doc.activeElement, exportButton);
  withFile.page.destroy();

  const inMemory = rig({ summaries: [s], conversations: { [s.id]: conv }, exportResult: { path: "", markdown: "# Two" } });
  await openFirst(inMemory);
  inMemory.button("Export")!.click();
  const aside2 = [...inMemory.page.element.querySelectorAll<HTMLElement>(".ui2-chats-aside")].find((a) => !a.hidden)!;
  [...aside2.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Markdown")!.click();
  await inMemory.settle();
  assert.match(aside2.textContent ?? "", /Ready\. Nothing was saved to disk yet\./);
  const saveAs = [...aside2.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Save as...")!;
  saveAs.click();
  await inMemory.settle();
  assert.match(aside2.textContent ?? "", /Saving a file is not available in this window/);
  // With a save dialog the file is written exactly once, with the prepared text.
  const writes: string[] = [];
  Object.assign(inMemory.dom.window, {
    showSaveFilePicker: async () => ({ createWritable: async () => ({ write: async (v: string) => { writes.push(v); }, close: async () => {} }) }),
  });
  saveAs.click();
  await inMemory.settle();
  assert.deepEqual(writes, ["# Two"]);
  assert.match(aside2.textContent ?? "", /The file is not encrypted\./);
  inMemory.page.destroy();

  const failing = rig({ summaries: [s], conversations: { [s.id]: conv }, failExport: true });
  await openFirst(failing);
  failing.button("Export")!.click();
  const aside3 = [...failing.page.element.querySelectorAll<HTMLElement>(".ui2-chats-aside")].find((a) => !a.hidden)!;
  [...aside3.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Markdown")!.click();
  await failing.settle();
  assert.match(aside3.textContent ?? "", /Could not export this chat\. Nothing was changed\. Try again\./);
  failing.page.destroy();
});

test("a chat that cannot be opened says so and goes back; Back restores the list and focus", async () => {
  const s = summary(1);
  const bad = rig({ summaries: [s], failRead: true });
  await openFirst(bad);
  assert.match(bad.page.element.textContent ?? "", /Could not open this chat/);
  bad.button("Back to Archive")!.click();
  assert.equal(bad.page.element.querySelector<HTMLElement>(".ui2-chats-reader"), null);
  assert.deepEqual(bad.details.at(-1), null, "the shell is told the chat was closed");
  bad.page.destroy();

  const { s: s2, conv } = oneChat();
  const r = rig({ summaries: [s2], conversations: { [s2.id]: conv } });
  await r.settle();
  const row = r.rows()[0]!;
  row.focus();
  row.click();
  await r.settle();
  r.page.element.querySelector<HTMLButtonElement>(".ui2-chats-back")!.click();
  assert.equal(r.doc.activeElement, row, "focus returns to the chat that was open");
  assert.equal(r.page.element.querySelector(".ui2-chats-reader"), null);
  r.page.destroy();
});

test("a language change repaints list and reader in place without reading the archive again", async () => {
  const { s, conv } = oneChat({ completeness: "partial" });
  const r = rig({ summaries: [s], conversations: { [s.id]: conv } });
  await r.settle();
  assert.match(r.page.element.textContent ?? "", /Everything you saved or imported/);
  r.i18n.setLocale("id");
  assert.match(r.page.element.textContent ?? "", /Semua yang kamu simpan atau impor/);
  assert.ok(r.button("Impor"));
  assert.match(r.rows()[0]!.textContent ?? "", /disimpan.*Parsial/s);
  r.rows()[0]!.click();
  await r.settle();
  const readsBefore = r.calls.filter((c) => c.command === "read_conversation").length;
  r.i18n.setLocale("en");
  assert.ok(r.button("Info"));
  assert.ok(r.button("Export"));
  assert.ok(r.button("Why?"));
  r.i18n.setLocale("id");
  assert.ok(r.button("Ekspor"));
  assert.ok(r.button("Kenapa?"));
  assert.match(r.page.element.textContent ?? "", /Salinan ini mungkin kehilangan beberapa pesan/);
  assert.equal(r.calls.filter((c) => c.command === "read_conversation").length, readsBefore);
  assert.match(r.page.element.querySelector(".ui2-chats-user")?.textContent ?? "", /this chat is for mind-place save testing/, "chat text is never translated");
  r.page.destroy();
});

test("late answers after leaving are dropped: nothing is drawn and nothing throws", async () => {
  const gate = (() => { let release!: () => void; const promise = new Promise<void>((resolve) => { release = resolve; }); return { promise, release }; })();
  const slowList = rig({ summaries: [summary(1)], listGate: gate.promise });
  slowList.page.destroy();
  gate.release();
  await slowList.settle();
  assert.equal(slowList.page.element.childElementCount, 0);

  const { s, conv } = oneChat();
  const readGate = (() => { let release!: () => void; const promise = new Promise<void>((resolve) => { release = resolve; }); return { promise, release }; })();
  const r = rig({ summaries: [s], conversations: { [s.id]: conv }, readGate: readGate.promise });
  await r.settle();
  r.rows()[0]!.click();
  assert.ok(r.page.element.querySelector('[aria-busy="true"]'), "reader shows loading");
  r.page.element.querySelector<HTMLButtonElement>(".ui2-chats-back")!.click();
  readGate.release();
  await r.settle();
  assert.equal(r.page.element.querySelector(".ui2-chats-reader"), null, "a closed chat is not redrawn");
  r.page.destroy();
});

test("inside the shell: breadcrumb reads Home > Archive > title, the page owns the heading, Back and sidebar return to the list", async () => {
  const dom = new JSDOM("<!doctype html><body><div id='root'></div></body>", { pretendToBeVisual: true, url: "https://shell.test/" });
  const doc = dom.window.document;
  Object.assign(globalThis, { document: doc, window: dom.window });
  const { s, conv } = oneChat();
  const invoke: Invoke = async <T>(command: string, args: Record<string, unknown> = {}) => {
    if (command === "list_conversations") return [s] as T;
    if (command === "read_conversation") return conv as T;
    if (command === "list_snapshots") return [] as T;
    throw new Error(`unexpected ${command} ${JSON.stringify(args)}`);
  };
  const i18n = createI18nController({ initialLocale: "en", storage: { getItem: () => null, setItem: () => {} } });
  const shell = mountUi2Shell(doc.getElementById("root")!, { onLock: () => {}, i18n, pages: createUi2Pages({ invoke }) });
  const settle = async () => { for (let i = 0; i < 10; i += 1) await new Promise((resolve) => setTimeout(resolve, 0)); };
  shell.navigate("/archive");
  await settle();
  assert.equal(doc.querySelectorAll("h1").length, 1);
  assert.equal(doc.querySelector("h1")?.textContent, "Archive");
  const crumbs = () => [...doc.querySelectorAll(".ui2-breadcrumb-item")].map((c) => c.textContent);
  assert.deepEqual(crumbs(), ["Home", "Archive"]);

  doc.querySelector<HTMLButtonElement>("button.ui2-list-row")!.click();
  await settle();
  assert.deepEqual(crumbs(), ["Home", "Archive", "Mind-place save testing"]);
  assert.equal(doc.querySelectorAll("h1").length, 1, "one heading: the chat title");
  assert.equal(doc.querySelector("h1")?.textContent, "Mind-place save testing");
  assert.equal(doc.activeElement, doc.querySelector("h1"), "focus lands on the chat title");
  assert.equal(shell.getRoute().path, "/archive", "no chat id in the route");
  assert.doesNotMatch(dom.window.location.href, /src:v1:|Mind-place/);

  // The Archive crumb goes back to the list.
  doc.querySelectorAll<HTMLButtonElement>(".ui2-breadcrumb-link")[1]!.click();
  assert.deepEqual(crumbs(), ["Home", "Archive"]);
  assert.equal(doc.querySelector("h1")?.textContent, "Archive");
  assert.equal(doc.activeElement?.textContent?.includes("Mind-place save testing"), true, "focus returns to the chat row");

  // Choosing Archive in the sidebar while a chat is open also returns to the list.
  doc.querySelector<HTMLButtonElement>("button.ui2-list-row")!.click();
  await settle();
  doc.querySelector<HTMLButtonElement>('[data-ui2-route="/archive"]')!.click();
  assert.deepEqual(crumbs(), ["Home", "Archive"]);

  // Leaving the destination destroys the page and any open chat.
  doc.querySelector<HTMLButtonElement>("button.ui2-list-row")!.click();
  await settle();
  shell.navigate("/memory");
  assert.equal(doc.querySelector(".ui2-chats-reader"), null);
  assert.deepEqual(crumbs(), ["Home", "Memory"]);
  shell.destroy();
});

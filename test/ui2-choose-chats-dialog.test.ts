import assert from "node:assert/strict";
import test from "node:test";
import { JSDOM } from "jsdom";
import type { GrantableChat } from "../src/mcp-read-grants.js";
import type { Invoke } from "../src/reader.js";
import { openChooseChatsDialog } from "../src/ui2/connections/choose-chats-dialog.js";
import { createI18nController } from "../src/ui2/i18n/index.js";

const sid = (n: number) => `src:v1:${n.toString(16).padStart(64, "0")}`;
const chat = (n: number, granted = false, provider = "chatgpt"): GrantableChat => ({
  source_id: sid(n),
  title: `Plan ${n}`,
  provider,
  captured_at: "2026-10-18T10:00:00Z",
  granted,
});

interface Call {
  command: string;
  args: Record<string, unknown>;
}

function setup(options: { chats?: GrantableChat[]; failList?: boolean; failSave?: boolean; locale?: "en" | "id" } = {}) {
  const dom = new JSDOM("<!doctype html><body><button id=\"opener\">open</button></body>", {
    pretendToBeVisual: true,
  });
  const doc = dom.window.document;
  Object.assign(globalThis, { document: doc, window: dom.window });
  const chats = options.chats ?? [chat(1, true), chat(2), chat(3, false, "claude")];
  const calls: Call[] = [];
  const invoke: Invoke = async <T>(command: string, args: Record<string, unknown> = {}) => {
    calls.push({ command, args });
    if (command === "list_local_mcp_grantable_chats") {
      if (options.failList) throw new Error("MP_X");
      const query = String(args.query ?? "").toLowerCase();
      const rows = chats.filter((c) => c.title.toLowerCase().includes(query));
      const offset = Number(args.offset ?? 0);
      const limit = Number(args.limit ?? 25);
      return {
        total: rows.length,
        allowed: rows.filter((c) => c.granted).length,
        rows: rows.slice(offset, offset + limit),
      } as T;
    }
    if (options.failSave) throw new Error("MP_X");
    return ((args.sourceIds as string[] | undefined)?.length ?? 0) as T;
  };
  const i18n = createI18nController({
    initialLocale: options.locale ?? "en",
    storage: { getItem: () => null, setItem: () => {} },
  });
  const opener = doc.getElementById("opener") as HTMLButtonElement;
  let saved = 0;
  const dialog = openChooseChatsDialog({
    doc,
    i18n,
    invoke,
    client: { id: "client:v1:abc", displayName: "Claude Code" },
    opener,
    onSaved: () => {
      saved += 1;
    },
  });
  const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
  return { dom, doc, calls, dialog, opener, tick, savedCount: () => saved };
}

const mutations = (calls: Call[]) => calls.filter((c) => c.command !== "list_local_mcp_grantable_chats");

test("lists saved chats with one switch each, shows current access, and never renders ids", async () => {
  const { doc, tick, dialog } = setup();
  await tick();
  const text = dialog.dialog.textContent ?? "";
  assert.match(text, /Choose chats for Claude Code/);
  assert.match(text, /Plan 1/);
  assert.match(text, /ChatGPT/);
  assert.match(text, /Claude/);
  assert.match(text, /1 of 3 chats allowed/);
  const switches = [...doc.querySelectorAll<HTMLButtonElement>("[role=switch]")];
  assert.equal(switches.length, 3);
  assert.deepEqual(switches.map((s) => s.getAttribute("aria-checked")), ["true", "false", "false"]);
  assert.equal(switches[1].getAttribute("aria-label"), "Allow Plan 2");
  // Technical identifiers are never visible or in accessible names.
  assert.doesNotMatch(doc.body.innerHTML, /src:v1:/);
  assert.doesNotMatch(doc.body.innerHTML, /client:v1:/);
});

test("nothing is changed before Review is approved; approving saves grants and removals once", async () => {
  const { doc, calls, tick, dialog, savedCount, opener } = setup();
  await tick();
  const switches = () => [...doc.querySelectorAll<HTMLButtonElement>("[role=switch]")];
  const buttons = () => [...dialog.dialog.querySelectorAll<HTMLButtonElement>("button.ui2-btn")];
  const labelled = (label: string) => buttons().find((b) => b.textContent === label)!;
  assert.equal(labelled("Review changes").disabled, true, "no changes yet");
  switches()[1].click(); // allow Plan 2
  switches()[0].click(); // remove Plan 1
  assert.equal(labelled("Review changes").disabled, false);
  assert.equal(mutations(calls).length, 0, "choosing alone changes nothing");
  labelled("Review changes").click();
  const review = dialog.dialog.textContent ?? "";
  assert.match(review, /Review before saving/);
  assert.match(review, /Will be allowed: 1 chat/);
  assert.match(review, /Will be removed: 1 chat/);
  assert.match(review, /Plan 2/);
  assert.equal(mutations(calls).length, 0, "Review step still changes nothing");
  labelled("Back").click();
  assert.equal(switches().length, 3, "Back returns to Choose");
  labelled("Review changes").click();
  labelled("Approve and save").click();
  await tick();
  await tick();
  assert.deepEqual(
    mutations(calls).map((c) => [c.command, c.args.sourceIds]),
    [
      ["grant_local_mcp_client_sources", [sid(2)]],
      ["revoke_local_mcp_client_sources", [sid(1)]],
    ],
  );
  assert.ok(mutations(calls).every((c) => c.args.clientId === "client:v1:abc"));
  assert.equal(savedCount(), 1);
  assert.equal(dialog.isOpen(), false);
  assert.equal(doc.activeElement, opener, "focus returns to the opener");
});

test("a failed save keeps the dialog open, says nothing changed, and does not report success", async () => {
  const { doc, tick, dialog, savedCount } = setup({ failSave: true });
  await tick();
  doc.querySelectorAll<HTMLButtonElement>("[role=switch]")[1].click();
  const buttons = () => [...dialog.dialog.querySelectorAll<HTMLButtonElement>("button.ui2-btn")];
  buttons().find((b) => b.textContent === "Review changes")!.click();
  buttons().find((b) => b.textContent === "Approve and save")!.click();
  await tick();
  await tick();
  assert.match(dialog.dialog.textContent ?? "", /Could not save\. Nothing was changed\. Try again\./);
  assert.equal(dialog.isOpen(), true);
  assert.equal(savedCount(), 0);
  assert.equal(buttons().find((b) => b.textContent === "Approve and save")!.disabled, false);
});

test("empty and error states are plain words, not blank or zero", async () => {
  const empty = setup({ chats: [] });
  await empty.tick();
  assert.match(empty.dialog.dialog.textContent ?? "", /No saved chats yet/);
  const failed = setup({ failList: true });
  await failed.tick();
  const text = failed.dialog.dialog.textContent ?? "";
  assert.match(text, /Could not load your chats/);
  assert.match(text, /Your chats were not changed/);
  assert.doesNotMatch(text, /0 of 0/);
});

test("remove all access is an inline two-step and says chats are never deleted", async () => {
  const { calls, tick, dialog } = setup();
  await tick();
  const trigger = [...dialog.dialog.querySelectorAll<HTMLButtonElement>("button")].find(
    (b) => b.textContent === "Remove all access",
  )!;
  trigger.click();
  assert.match(dialog.dialog.textContent ?? "", /Your chats are never deleted\./);
  assert.equal(mutations(calls).length, 0, "the first click only asks");
  const confirm = [...dialog.dialog.querySelectorAll<HTMLButtonElement>("button")].find(
    (b) => b.textContent === "Remove access",
  )!;
  confirm.click();
  await tick();
  await tick();
  assert.deepEqual(mutations(calls).map((c) => c.command), ["revoke_all_local_mcp_client_sources"]);
});

test("Indonesian copy ships with English, product names stay untranslated, Escape closes", async () => {
  const { tick, dialog, dom } = setup({ locale: "id" });
  await tick();
  const text = dialog.dialog.textContent ?? "";
  assert.match(text, /Pilih obrolan untuk Claude Code/);
  assert.match(text, /1 dari 3 obrolan diizinkan/);
  assert.match(text, /ChatGPT/);
  assert.doesNotMatch(text, /Choose chats|chats allowed|Pilih chat/);
  dialog.dialog.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  assert.equal(dialog.isOpen(), false);
});

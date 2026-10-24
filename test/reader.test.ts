import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";
import { JSDOM } from "jsdom";
import { afterEach, test } from "node:test";
import { mountReader, renderMessageContent, type Invoke } from "../src/reader.js";
import type { Conversation, ConversationSummary } from "../src/contracts.js";

const mounted: Array<ReturnType<typeof mountReader>> = [];

afterEach(() => {
  while (mounted.length) mounted.pop()?.destroy();
});

function summary(id: string, title: string): ConversationSummary {
  return {
    id,
    conversation_id: `provider-${id}`,
    provider: "chatgpt",
    title,
    source_url: `https://chatgpt.com/c/${id}`,
    captured_at: "2026-10-15T05:00:00.000Z",
    message_count: 2,
    snapshot_count: 1,
    completeness: "complete",
    warnings: [],
  };
}

function conversation(item: ConversationSummary, answer: string): Conversation {
  return {
    summary: item,
    capture: {
      schema_version: 1,
      provider: "chatgpt",
      conversation_id: item.conversation_id,
      source_url: item.source_url,
      title: item.title,
      captured_at: item.captured_at,
      capture_method: "manual-dom",
      completeness: item.completeness,
      warnings: item.warnings,
      messages: [
        { id: `${item.id}-u`, role: "user", text: "Question", format: "plain", attachments: [] },
        { id: `${item.id}-a`, role: "assistant", text: answer, format: "markdown", attachments: [] },
      ],
    },
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

function createReader(invoke: Invoke, copied: string[] = []) {
  const dom = new JSDOM("<!doctype html><html><body><div id='app'></div></body></html>", {
    url: "https://archive.test/",
  });
  const root = dom.window.document.querySelector<HTMLElement>("#app")!;
  const app = mountReader({
    root,
    invoke,
    clipboard: { writeText: async (value) => { copied.push(value); } },
  });
  mounted.push(app);
  return { dom, root, app, copied };
}

test("archived Markdown and HTML cannot execute scripts or load remote images", () => {
  const dom = new JSDOM("<!doctype html><html><body></body></html>", { url: "https://archive.test/" });
  const content = renderMessageContent(dom.window.document, {
    format: "markdown",
    text: `<h2 onclick="alert(1)">Saved answer</h2>
<script>window.compromised = true</script>
<img src="https://tracker.example/pixel.png" alt="remote pixel">
<div style="background-image:url(https://tracker.example/leak)">text</div>
<a href="javascript:alert(2)">unsafe link</a>
<a href="https://example.com/reference">safe reference</a>`,
  });

  assert.match(content.textContent ?? "", /Saved answer/);
  assert.equal(content.querySelector("script"), null);
  assert.equal(content.querySelector("img"), null);
  assert.equal(content.querySelector("[onclick], [style]"), null);
  const unsafe = [...content.querySelectorAll("a")].find((link) => link.textContent === "unsafe link");
  assert.ok(unsafe);
  assert.equal(unsafe.getAttribute("href"), null);
  const safe = [...content.querySelectorAll("a")].find((link) => link.textContent === "safe reference");
  assert.equal(safe?.getAttribute("href"), "https://example.com/reference");
  assert.equal(safe?.getAttribute("rel"), "noopener noreferrer");
});

test("conversation dates show original activity per snapshot while capture time remains audit metadata", async () => {
  const id = "9".repeat(64);
  const item = {
    ...summary(id, "Dated conversation"),
    captured_at: "2026-10-15T12:00:00Z",
    conversation_date: "2025-10-15T12:00:00Z",
    date_source: "provider-updated" as const,
  };
  const undated = summary("8".repeat(64), "No original date");
  const invoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
    if (command === "list_conversations") return [item, undated] as T;
    if (command === "list_snapshots") return [
      { snapshot_id: "latest", captured_at: item.captured_at, message_count: 2, completeness: "complete" },
      { snapshot_id: "older", captured_at: "2026-10-14T12:00:00Z", message_count: 2, completeness: "complete" },
    ] as T;
    if (command === "read_conversation") {
      if (args?.id === undated.id) return conversation(undated, "No dated source data") as T;
      const older = args?.snapshotId === "older";
      const result = conversation(item, older ? "Older revision" : "Current revision");
      result.snapshot_id = older ? "older" : "latest";
      result.capture.captured_at = older ? "2026-10-14T12:00:00Z" : item.captured_at;
      result.capture.conversation_created_at = "2020-02-03T04:05:00Z";
      result.summary = {
        ...item,
        conversation_date: older ? "2023-04-05T06:07:00Z" : item.conversation_date,
        date_source: older ? "message-authored" : item.date_source,
      };
      return result as T;
    }
    throw new Error(`Unexpected command: ${command}`);
  };
  const { dom, root, app } = createReader(invoke);
  await app.ready;
  await delay(0);

  const datedRow = root.querySelector(`[data-conversation-id="${id}"] .conversation-meta`);
  const undatedRow = root.querySelector(`[data-conversation-id="${undated.id}"] .conversation-meta`);
  assert.match(datedRow?.textContent ?? "", /2025/);
  assert.doesNotMatch(datedRow?.textContent ?? "", /2026/);
  assert.match(undatedRow?.textContent ?? "", /Date unavailable/);
  assert.match(root.querySelector(".conversation-details")?.textContent ?? "", /Updated/);
  assert.match(root.querySelector(".conversation-details")?.textContent ?? "", /Created/);
  assert.match(root.querySelector(".conversation-details")?.textContent ?? "", /Captured/);

  root.querySelector<HTMLButtonElement>(`[data-conversation-id="${undated.id}"]`)!.click();
  await delay(0);
  assert.match(root.querySelector(".conversation-details")?.textContent ?? "", /Date unavailable/);

  root.querySelector<HTMLButtonElement>(`[data-conversation-id="${id}"]`)!.click();
  await delay(0);
  const selector = root.querySelector<HTMLSelectElement>("#capture-snapshot")!;
  selector.value = "older";
  selector.dispatchEvent(new dom.window.Event("change", { bubbles: true }));
  await delay(0);
  const headingDetails = root.querySelector(".conversation-details")?.textContent ?? "";
  assert.match(headingDetails, /Last message/);
  assert.match(headingDetails, /2023/);
  assert.match(headingDetails, /Captured/);
  assert.doesNotMatch(headingDetails, /Updated/);
});

test("undated Gemini uses its captured time in the list, without claiming provider activity time", async () => {
  const id = "7".repeat(64);
  const gemini: ConversationSummary = {
    ...summary(id, "Gemini latest"),
    provider: "gemini",
    source_url: "https://gemini.google.com/app/AbCd1234efGh5678",
    captured_at: "2026-10-24T07:53:49Z",
  };
  const unknown = summary("6".repeat(64), "Other undated conversation");
  const invoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
    if (command === "list_conversations") return [gemini, unknown] as T;
    if (command === "list_snapshots") return [] as T;
    if (command === "read_conversation") return conversation(args?.id === gemini.id ? gemini : unknown, "Saved answer") as T;
    throw new Error(`Unexpected command: ${command}`);
  };
  const { root, app } = createReader(invoke);
  await app.ready;
  await delay(0);

  const geminiMeta = root.querySelector(`[data-conversation-id="${id}"] .conversation-meta`);
  const undatedMeta = root.querySelector(`[data-conversation-id="${unknown.id}"] .conversation-meta`);
  assert.match(geminiMeta?.textContent ?? "", /Captured.*2026.*messages/);
  assert.doesNotMatch(geminiMeta?.textContent ?? "", /Date unavailable/);
  assert.match(undatedMeta?.textContent ?? "", /Date unavailable/);
  assert.doesNotMatch(root.querySelector(".conversation-details")?.textContent ?? "", /Date unavailable/);
  assert.match(root.querySelector(".conversation-details")?.textContent ?? "", /Captured/);
});

test("the history selector and export follow the opened snapshot even when it is older than the newest timestamp", async () => {
  const item = summary("d".repeat(64), "Revisited capture");
  const older = "1".repeat(64), newer = "2".repeat(64);
  let exported: Record<string, unknown> | undefined;
  const invoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
    if (command === "list_conversations") return [item] as T;
    if (command === "list_snapshots") return [
      { snapshot_id: newer, captured_at: "2026-10-15T06:00:00Z", message_count: 2, completeness: "partial" },
      { snapshot_id: older, captured_at: "2026-10-15T05:00:00Z", message_count: 2, completeness: "partial" },
    ] as T;
    if (command === "read_conversation") return { ...conversation(item, "The reopened older answer."), snapshot_id: older } as T;
    if (command === "export_markdown") { exported = args; return { path: "archive.md", markdown: "The reopened older answer." } as T; }
    throw new Error(command);
  };
  const { root, app } = createReader(invoke);
  await app.ready; await delay(0);
  assert.equal(root.querySelector<HTMLSelectElement>("#capture-snapshot")?.value, older);
  root.querySelector<HTMLButtonElement>("[data-action='export-markdown']")!.click();
  await delay(0);
  assert.equal(exported?.snapshotId, older);
});

test("refresh reloads the selected conversation after a new manual capture", async () => {
  let revision = 1;
  const id = "e".repeat(64);
  const invoke: Invoke = async <T>(command: string) => {
    const item = { ...summary(id, "Saved again"), snapshot_count: revision, captured_at: `2026-10-15T0${revision}:00:00Z` };
    if (command === "list_conversations") return [item] as T;
    if (command === "list_snapshots") return [] as T;
    if (command === "read_conversation") return conversation(item, revision === 1 ? "Earlier answer" : "New manually saved answer") as T;
    throw new Error(command);
  };
  const { root, app } = createReader(invoke);
  await app.ready; await delay(0);
  assert.match(root.textContent ?? "", /Earlier answer/);
  revision = 2;
  await app.refresh(); await delay(0);
  assert.match(root.textContent ?? "", /New manually saved answer/);
  assert.doesNotMatch(root.textContent ?? "", /Earlier answer/);
});

test("explicit refresh follows A to B to A even when all sidebar metadata stays equal", async () => {
  const item = { ...summary("f".repeat(64), "Same metadata"), snapshot_count: 2 };
  let current = "A", reads = 0;
  const invoke: Invoke = async <T>(command: string) => {
    if (command === "list_conversations") return [item] as T;
    if (command === "list_snapshots") return [] as T;
    if (command === "read_conversation") { reads++; return { ...conversation(item, `Snapshot ${current}`), snapshot_id: current === "A" ? "1".repeat(64) : "2".repeat(64) } as T; }
    throw new Error(command);
  };
  const { root, app } = createReader(invoke);
  await app.ready; await delay(0);
  assert.match(root.textContent ?? "", /Snapshot A/);
  current = "B"; await app.refresh(); await delay(0);
  assert.match(root.textContent ?? "", /Snapshot B/);
  current = "A"; await app.refresh(); await delay(0);
  assert.match(root.textContent ?? "", /Snapshot A/);
  assert.equal(reads, 3);
});

test("a late conversation read cannot replace the current selection", async () => {
  const first = summary("a".repeat(64), "First archive");
  const second = summary("b".repeat(64), "Second archive");
  const firstRead = deferred<Conversation>();
  const secondRead = deferred<Conversation>();
  const invoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
    if (command === "list_conversations") return [first, second] as T;
    if (command === "read_conversation") {
      return (args?.id === first.id ? firstRead.promise : secondRead.promise) as Promise<T>;
    }
    throw new Error(`Unexpected command: ${command}`);
  };
  const { dom, root, app } = createReader(invoke);
  await app.ready;

  const secondRow = root.querySelector<HTMLButtonElement>(`[data-conversation-id="${second.id}"]`);
  assert.ok(secondRow, "the second archive is selectable");
  secondRow.click();
  secondRead.resolve(conversation(second, "The second transcript is current."));
  await delay(0);
  assert.match(root.textContent ?? "", /The second transcript is current\./);

  firstRead.resolve(conversation(first, "The stale first transcript must not return."));
  await delay(0);
  assert.match(root.textContent ?? "", /The second transcript is current\./);
  assert.doesNotMatch(root.textContent ?? "", /The stale first transcript must not return\./);
  assert.equal(dom.window.document.querySelector("textarea"), null);
});

test("the reader has no chat composer and copy/export use archive actions", async () => {
  const item = summary("c".repeat(64), "Notes from a conversation");
  const invokeCalls: Array<{ command: string; args?: Record<string, unknown> }> = [];
  const invoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
    invokeCalls.push({ command, args });
    if (command === "list_conversations") return [item] as T;
    if (command === "read_conversation") return conversation(item, "A **useful** answer.") as T;
    if (command === "export_markdown") return { path: "C:/Mind Place/exports/notes.md", markdown: "# Notes\n\nA useful answer." } as T;
    throw new Error(`Unexpected command: ${command}`);
  };
  const { root, app, copied } = createReader(invoke);
  await app.ready;
  await delay(0);

  assert.equal(root.querySelector("textarea"), null);
  assert.equal(root.querySelector("[data-testid='message-composer']"), null);
  const buttonNames = [...root.querySelectorAll("button")].map((button) => button.textContent?.trim() ?? "");
  assert.equal(buttonNames.some((name) => /^(new chat|send)$/i.test(name)), false);

  root.querySelector<HTMLButtonElement>("[data-action='copy-answer']")?.click();
  await delay(0);
  assert.deepEqual(copied, ["A useful answer."]);

  root.querySelector<HTMLButtonElement>("[data-action='export-markdown']")?.click();
  await delay(0);
  assert.ok(invokeCalls.some((call) => call.command === "export_markdown" && call.args?.id === item.id));
  assert.match(root.textContent ?? "", /C:\/Mind Place\/exports\/notes\.md/);
  root.querySelector<HTMLButtonElement>("[data-action='copy-markdown']")?.click();
  await delay(0);
  assert.deepEqual(copied, ["A useful answer.", "# Notes\n\nA useful answer."]);
});

test("capture history opens one selected snapshot without merging transcripts", async () => {
  const item = summary("d".repeat(64), "Revised conversation");
  const latest = { snapshot_id: "latest", captured_at: "2026-10-15T05:00:00.000Z", message_count: 2, completeness: "partial" as const };
  const earlier = { snapshot_id: "earlier", captured_at: "2026-10-14T05:00:00.000Z", message_count: 1, completeness: "complete" as const };
  const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
  const invoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
    calls.push({ command, args });
    if (command === "list_conversations") return [item] as T;
    if (command === "list_snapshots") return [latest, earlier] as T;
    if (command === "read_conversation") {
      return conversation(item, args?.snapshotId === "earlier" ? "Only the earlier answer." : "Only the latest answer.") as T;
    }
    if (command === "export_markdown") return { path: "C:/Mind Place/exports/earlier.md", markdown: "Only the earlier answer." } as T;
    throw new Error(`Unexpected command: ${command}`);
  };
  const { dom, root, app } = createReader(invoke);
  await app.ready;
  await delay(0);

  const selector = root.querySelector<HTMLSelectElement>("#capture-snapshot");
  assert.ok(selector);
  assert.equal(selector.options.length, 2);
  assert.equal(selector.value, "latest");
  assert.match(root.textContent ?? "", /Only the latest answer\./);

  selector.value = "earlier";
  selector.dispatchEvent(new dom.window.Event("change", { bubbles: true }));
  await delay(0);
  assert.ok(calls.some((call) => call.command === "read_conversation" && call.args?.snapshotId === "earlier"));
  assert.match(root.textContent ?? "", /Only the earlier answer\./);
  assert.doesNotMatch(root.textContent ?? "", /Only the latest answer\./);
  root.querySelector<HTMLButtonElement>("[data-action='export-markdown']")?.click();
  await delay(0);
  assert.ok(calls.some((call) => call.command === "export_markdown" && call.args?.snapshotId === "earlier"));
});

test("stale search results and captured-image reads cannot overwrite newer UI state", async () => {
  const first = summary("e".repeat(64), "Earlier match");
  const second = summary("f".repeat(64), "Current match");
  const staleSearch = deferred<ConversationSummary[]>();
  const currentSearch = deferred<ConversationSummary[]>();
  const staleAsset = deferred<{ mime_type: string; data_url: string }>();
  const currentAsset = deferred<{ mime_type: string; data_url: string }>();
  const invoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
    if (command === "list_conversations") {
      if (!args?.query) return [first] as T;
      if (args.query === "earlier") return staleSearch.promise as Promise<T>;
      if (args.query === "current") return currentSearch.promise as Promise<T>;
    }
    if (command === "list_snapshots") return [] as T;
    if (command === "read_conversation") {
      const selected = args?.id === first.id ? first : second;
      const result = conversation(selected, selected.id === first.id ? "Old answer." : "Current answer.");
      result.capture.messages.push({
        id: `${selected.id}-image`, role: "assistant", text: "Image attached", format: "plain",
        attachments: [{ id: "image", name: selected.id === first.id ? "Old image" : "Current image", mime_type: "image/png", asset_path: "image.png" }],
      });
      return result as T;
    }
    if (command === "get_asset") return (args?.id === first.id ? staleAsset.promise : currentAsset.promise) as Promise<T>;
    throw new Error(`Unexpected command: ${command}`);
  };
  const { dom, root, app } = createReader(invoke);
  await app.ready;
  await delay(0);

  const search = root.querySelector<HTMLInputElement>("#archive-search")!;
  search.value = "earlier";
  search.dispatchEvent(new dom.window.Event("input", { bubbles: true }));
  search.value = "current";
  search.dispatchEvent(new dom.window.Event("input", { bubbles: true }));
  currentSearch.resolve([second]);
  await delay(0);
  staleSearch.resolve([first]);
  await delay(0);
  assert.ok(root.querySelector(`[data-conversation-id="${second.id}"]`));
  assert.equal(root.querySelector(`[data-conversation-id="${first.id}"]`), null);

  const secondRow = root.querySelector<HTMLButtonElement>(`[data-conversation-id="${second.id}"]`)!;
  secondRow.click();
  await delay(0);
  currentAsset.resolve({ mime_type: "image/png", data_url: "data:image/png;base64,aGVsbG8=" });
  await delay(0);
  assert.equal(root.querySelector<HTMLImageElement>(".captured-image")?.alt, "Current image");
  staleAsset.resolve({ mime_type: "image/png", data_url: "data:image/png;base64,aGVsbG8=" });
  await delay(0);
  assert.equal(root.querySelector<HTMLImageElement>(".captured-image")?.alt, "Current image");
});


test("Settings tabs preserve explicit consent, keyboard navigation and focus restoration", async () => {
  const commands: string[] = [];
  const invoke: Invoke = async <T>(command: string) => {
    commands.push(command);
    if (command === "list_conversations") return [] as T;
    if (command === "get_setup") return {
      archive_path: "C:\\MindPlace\\archive", extension_path: "C:\\extension",
      host_path: "C:\\host", extension_id: "ext", native_host_name: "host",
      registered_browsers: [], platform: "Windows", legacy_available: false,
    } as T;
    if (command === "list_document_roots" || command === "list_local_mcp_clients") return [] as T;
    if (command === "get_local_mcp_status") return { running: false, endpoint: "" } as T;
    if (command === "get_hosted_tunnel_status") return { running: false, tunnel_id: null, connection_verified: false } as T;
    throw new Error("unexpected command: " + command);
  };
  const { dom, root, app } = createReader(invoke);
  await app.ready;
  const documents = root.querySelector<HTMLButtonElement>("[data-action='open-document-search']")!;
  documents.focus();
  documents.click();
  await delay(0);
  await delay(0);
  assert.equal(root.querySelector<HTMLButtonElement>("#settings-tab-documents")?.getAttribute("aria-selected"), "true");
  assert.equal(root.querySelector<HTMLElement>("#settings-panel-documents")?.hidden, false);
  assert.equal(root.querySelector<HTMLElement>("#settings-panel-connections")?.hidden, true);
  assert.match(root.querySelector<HTMLElement>("#settings-panel-documents")?.textContent ?? "", /Stage/);
  assert.equal(commands.some((command) => command === "approve_document_root" || command === "refresh_document_root_index"), false);
  const nav = root.querySelector<HTMLButtonElement>("#settings-tab-documents")!;
  nav.focus();
  nav.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
  assert.equal(root.querySelector<HTMLButtonElement>("#settings-tab-connections")?.getAttribute("aria-selected"), "true");
  assert.equal(dom.window.document.activeElement?.id, "settings-tab-connections");
  root.querySelector<HTMLButtonElement>("[data-action='close-setup']")!.click();
  assert.equal(root.querySelector<HTMLDialogElement>("#setup-dialog")!.hasAttribute("open"), false);
  assert.equal(dom.window.document.activeElement, documents);
});

test("Settings contains only the selected panel at accessible tab focus", async () => {
  const invoke: Invoke = async <T>(command: string) => {
    if (command === "list_conversations") return [] as T;
    if (command === "get_setup") return {
      archive_path: "C:\\archive", extension_path: "C:\\extension", host_path: "C:\\host",
      extension_id: "ext", native_host_name: "host", registered_browsers: [],
      platform: "Windows", legacy_available: false,
    } as T;
    if (command === "list_document_roots" || command === "list_local_mcp_clients") return [] as T;
    if (command === "get_local_mcp_status") return { running: false, endpoint: "" } as T;
    if (command === "get_hosted_tunnel_status") return { running: false, tunnel_id: null, connection_verified: false } as T;
    throw Error(command);
  };
  const { dom, root, app } = createReader(invoke);
  await app.ready;
  root.querySelector<HTMLButtonElement>("[data-action='open-setup']")!.click();
  await delay(0);
  await delay(0);
  const panels = [...root.querySelectorAll<HTMLElement>("[data-settings-panel]")];
  assert.equal(panels.length, 4);
  assert.deepEqual(panels.filter(p => !p.hidden).map(p => p.dataset.settingsPanel), ["archive"]);
  // MP-7D generic import must remain reachable after reorganizing Settings.
  assert.ok(root.querySelector("#settings-panel-archive [data-generic-file='true']"));
  assert.ok(root.querySelector("#settings-panel-archive [data-action='import-generic']"));
  assert.ok(root.querySelector("#settings-panel-archive [data-action='register-chrome']"));
  const brave = root.querySelector<HTMLButtonElement>("#settings-panel-archive [data-action='register-brave']");
  assert.equal(brave?.textContent, "Register Brave");
  assert.ok(root.querySelector("#settings-panel-connections .setup-mcp"));
  const tab = root.querySelector<HTMLButtonElement>("#settings-tab-archive")!;
  tab.focus();
  tab.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "End", bubbles: true }));
  assert.deepEqual(panels.filter(p => !p.hidden).map(p => p.dataset.settingsPanel), ["privacy"]);
  assert.match(root.querySelector<HTMLElement>("#settings-panel-privacy")!.textContent ?? "", /Lock closes owner-only retrieval/);
});

test("Register Brave is an explicit Settings action and reports native host registration", async () => {
  let registeredBrowser: string | null = null;
  const invoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
    if (command === "list_conversations" || command === "list_document_roots" || command === "list_local_mcp_clients") return [] as T;
    if (command === "get_local_mcp_status") return { running: false, endpoint: "" } as T;
    if (command === "get_hosted_tunnel_status") return { running: false } as T;
    if (command === "get_setup" || command === "register_extension") {
      if (command === "register_extension") registeredBrowser = String(args?.browser);
      return {
        archive_path: "D:\\archive", extension_path: "D:\\extension", host_path: "D:\\host",
        extension_id: "test-extension", native_host_name: "com.mindplace.capture",
        registered_browsers: registeredBrowser ? [registeredBrowser] : [],
        platform: "windows", legacy_available: false,
      } as T;
    }
    throw Error(`Unexpected command ${command}`);
  };
  const { root, app } = createReader(invoke);
  await app.ready;
  root.querySelector<HTMLButtonElement>("[data-action='open-setup']")!.click();
  await delay(0);
  await delay(0);
  assert.equal(registeredBrowser, null, "Opening Settings must not register Brave automatically");
  root.querySelector<HTMLButtonElement>("[data-action='register-brave']")!.click();
  await delay(0);
  assert.equal(registeredBrowser, "brave");
  assert.match(root.querySelector<HTMLElement>(".registered-status")?.textContent ?? "", /Registered in Brave/);
  assert.equal(root.querySelector<HTMLButtonElement>("[data-action='register-chrome']")?.disabled, false);
});

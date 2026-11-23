import assert from "node:assert/strict";
import test from "node:test";
import { JSDOM } from "jsdom";
import type { Invoke } from "../src/reader.js";
import { createConnectionsPage } from "../src/ui2/connections/page.js";
import { createI18nController } from "../src/ui2/i18n/index.js";
import { mountUi2Shell } from "../src/ui2/shell/shell.js";
import { createUi2Pages } from "../src/ui2/shell/pages.js";

const sid = (n: number) => `src:v1:${n.toString(16).padStart(64, "0")}`;
const KEY = `mpc_v1_${"a".repeat(64)}`;

interface Client {
  id: string;
  display_name: string;
  state: string;
  policy_revision: number;
}

function rig(options: { running?: boolean; failShare?: boolean; failIssue?: boolean; failRegister?: boolean; registered?: string[]; tunnelError?: string; clipboard?: "ok" | "missing" | "reject" } = {}) {
  const dom = new JSDOM('<!doctype html><body><div id="root"></div></body>', { pretendToBeVisual: true });
  const doc = dom.window.document;
  Object.assign(globalThis, { document: doc, window: dom.window });
  const clients: Client[] = [
    { id: "client:v1:one", display_name: "Claude Code", state: "enabled", policy_revision: 2 },
    { id: "client:v1:two", display_name: "Empty app", state: "enabled", policy_revision: 1 },
    { id: "client:v1:old", display_name: "Old app", state: "revoked", policy_revision: 3 },
  ];
  const sources: Record<string, string[]> = { "client:v1:one": [sid(1), sid(2)], "client:v1:two": [] };
  let running = options.running ?? false;
  let registered = options.registered ?? ["chrome"];
  let tunnelRunning = false;
  const calls: Array<{ command: string; args: Record<string, unknown> }> = [];
  const invoke: Invoke = async <T>(command: string, args: Record<string, unknown> = {}) => {
    calls.push({ command, args });
    switch (command) {
      case "get_local_mcp_status":
        return { running, endpoint: "x" } as T;
      case "start_local_mcp":
      case "stop_local_mcp":
        if (options.failShare) throw new Error("MP_X");
        running = command === "start_local_mcp";
        return { running, endpoint: "x" } as T;
      case "list_local_mcp_clients":
        return clients.map((c) => ({ ...c })) as T;
      case "list_local_mcp_client_sources":
        return (sources[String(args.clientId)] ?? []) as T;
      case "issue_local_mcp_reader": {
        if (options.failIssue) throw new Error("MP_X");
        const id = `client:v1:new${clients.length}`;
        clients.push({ id, display_name: String(args.displayName), state: "enabled", policy_revision: 1 });
        sources[id] = [];
        return { client_id: id, display_name: args.displayName, credential: KEY, endpoint: "x" } as T;
      }
      case "revoke_local_mcp_client": {
        const client = clients.find((c) => c.id === args.clientId)!;
        client.state = "revoked";
        return undefined as T;
      }
      case "get_setup":
        return { registered_browsers: registered } as T;
      case "register_extension":
        if (options.failRegister) throw new Error("MP_X");
        registered = [...registered, String(args.browser)];
        return { registered_browsers: registered } as T;
      case "get_hosted_tunnel_status":
        return { running: tunnelRunning, tunnel_id: tunnelRunning ? "tunnel_secret_id" : null, connection_verified: false } as T;
      case "start_hosted_tunnel":
        if (options.tunnelError) throw new Error(options.tunnelError);
        tunnelRunning = true;
        return { running: true, tunnel_id: String(args.tunnelId), connection_verified: false } as T;
      case "stop_hosted_tunnel":
        tunnelRunning = false;
        return { running: false, tunnel_id: null, connection_verified: false } as T;
      case "list_local_mcp_grantable_chats":
        return { total: 0, allowed: 0, rows: [] } as T;
      default:
        throw new Error(`unexpected ${command}`);
    }
  };
  const i18n = createI18nController({ initialLocale: "en", storage: { getItem: () => null, setItem: () => {} } });
  const written: string[] = [];
  const page = createConnectionsPage({
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
  });
  doc.getElementById("root")!.append(page.element);
  const settle = async () => {
    for (let i = 0; i < 8; i += 1) await new Promise((resolve) => setTimeout(resolve, 0));
  };
  const button = (label: string) =>
    [...page.element.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === label);
  return { dom, doc, page, calls, i18n, settle, button, written, clients };
}

test("renders capture and AI access sections with honest status and no identifiers", async () => {
  const r = rig();
  await r.settle();
  const text = r.page.element.textContent ?? "";
  assert.match(text, /Save chats from your browser/);
  assert.match(text, /Saving options/);
  assert.match(text, /Set in the browser extension/);
  assert.match(text, /Let an AI app read your memory/);
  assert.match(text, /Sharing is off/);
  assert.doesNotMatch(text, /Connected/, "nothing claims a verified connection");
  assert.doesNotMatch(r.doc.body.innerHTML, /client:v1:|src:v1:|mpc_v1_/);
  assert.match(text, /Can read 2 chats/);
  assert.match(text, /Cannot read any chats yet/);
  assert.match(text, /Revoked apps \(1\)/);
});

test("sharing switch starts and stops the listener, and a failure leaves it unchanged with an error", async () => {
  const ok = rig();
  await ok.settle();
  const sw = () => ok.page.element.querySelector<HTMLButtonElement>("[role=switch]")!;
  assert.equal(sw().getAttribute("aria-checked"), "false");
  sw().click();
  await ok.settle();
  assert.ok(ok.calls.some((c) => c.command === "start_local_mcp"));
  assert.equal(sw().getAttribute("aria-checked"), "true");
  assert.match(ok.page.element.textContent ?? "", /Sharing is on/);
  const bad = rig({ failShare: true });
  await bad.settle();
  bad.page.element.querySelector<HTMLButtonElement>("[role=switch]")!.click();
  await bad.settle();
  assert.match(bad.page.element.textContent ?? "", /Could not change sharing\. Nothing was changed\. Try again\./);
  assert.equal(bad.page.element.querySelector<HTMLButtonElement>("[role=switch]")!.getAttribute("aria-checked"), "false");
});

test("create access shows the key once, copies it, survives a language switch, and is cleared on destroy", async () => {
  const r = rig();
  await r.settle();
  r.button("Create access")!.click();
  await r.settle();
  const issue = r.calls.find((c) => c.command === "issue_local_mcp_reader")!;
  assert.equal(issue.args.displayName, "My AI app");
  assert.ok((r.page.element.textContent ?? "").includes(KEY));
  assert.match(r.page.element.textContent ?? "", /shown only once/);
  assert.match(r.page.element.textContent ?? "", /My AI app/, "the new app appears in the list");
  r.button("Copy")!.click();
  await r.settle();
  assert.deepEqual(r.written, [KEY]);
  assert.ok(r.button("Copied"));
  r.i18n.setLocale("id");
  await r.settle();
  assert.ok((r.page.element.textContent ?? "").includes(KEY), "language switch keeps the one-time key");
  assert.match(r.page.element.textContent ?? "", /Buat akses/);
  r.page.destroy();
  assert.equal((r.doc.body.textContent ?? "").includes(KEY), false, "destroy clears the key from the page");
  assert.equal(r.page.element.childElementCount, 0);
});

test("copy never reports success when the clipboard is missing", async () => {
  const r = rig({ clipboard: "missing" });
  await r.settle();
  r.button("Create access")!.click();
  await r.settle();
  const copy = r.button("Copy")!;
  assert.ok(copy, "the Copy button is still present");
  assert.equal(copy.disabled, true, "cannot be clicked when nothing can be copied");
  copy.click();
  await r.settle();
  assert.equal(r.button("Copied"), undefined, "no false success");
  assert.match(r.page.element.textContent ?? "", /copy it yourself/);
  assert.ok((r.page.element.textContent ?? "").includes(KEY), "the key stays visible for manual copy");
});

test("copy never reports success when the clipboard rejects, and a later success still works", async () => {
  const r = rig({ clipboard: "reject" });
  await r.settle();
  r.button("Create access")!.click();
  await r.settle();
  r.button("Copy")!.click();
  await r.settle();
  assert.equal(r.button("Copied"), undefined, "no false success");
  assert.ok(r.button("Copy"), "button stays available for another try");
  assert.match(r.page.element.textContent ?? "", /Could not copy automatically/);
  assert.deepEqual(r.written, []);
  const ok = rig();
  await ok.settle();
  ok.button("Create access")!.click();
  await ok.settle();
  ok.button("Copy")!.click();
  await ok.settle();
  assert.ok(ok.button("Copied"));
  assert.doesNotMatch(ok.page.element.textContent ?? "", /copy it yourself/);
});

test("a failed create shows an error and never shows a key", async () => {
  const r = rig({ failIssue: true });
  await r.settle();
  r.button("Create access")!.click();
  await r.settle();
  assert.match(r.page.element.textContent ?? "", /Could not create access\. Try again\./);
  assert.equal((r.page.element.textContent ?? "").includes(KEY), false);
});

test("revoke is an inline two-step; revoking moves the app to the revoked list", async () => {
  const r = rig();
  await r.settle();
  const revokeButtons = () => [...r.page.element.querySelectorAll<HTMLButtonElement>("button")].filter((b) => b.textContent === "Revoke access");
  revokeButtons()[0].click();
  assert.match(r.page.element.textContent ?? "", /Turn off Claude Code's access\? It stops working right away\./);
  assert.equal(r.calls.some((c) => c.command === "revoke_local_mcp_client"), false, "first click only asks");
  const confirm = [...r.page.element.querySelectorAll<HTMLButtonElement>(".ui2-inline-confirm-box button")].find(
    (b) => b.textContent === "Revoke access",
  )!;
  confirm.click();
  await r.settle();
  assert.deepEqual(
    r.calls.filter((c) => c.command === "revoke_local_mcp_client").map((c) => c.args.clientId),
    ["client:v1:one"],
  );
  assert.match(r.page.element.textContent ?? "", /Revoked apps \(2\)/);
});

test("Choose chats opens the approved dialog for that app", async () => {
  const r = rig();
  await r.settle();
  r.button("Choose chats")!.click();
  await r.settle();
  assert.match(r.doc.body.textContent ?? "", /Choose chats for Claude Code/);
  assert.ok(r.calls.some((c) => c.command === "list_local_mcp_grantable_chats" && c.args.clientId === "client:v1:one"));
});

test("browser registration shows registered browsers and registers the others", async () => {
  const r = rig();
  await r.settle();
  assert.match(r.page.element.textContent ?? "", /Registered/);
  const registerButtons = [...r.page.element.querySelectorAll<HTMLButtonElement>("button")].filter((b) => b.textContent === "Register");
  assert.equal(registerButtons.length, 2, "Edge and Brave can be registered, Chrome already is");
  registerButtons[0].click();
  await r.settle();
  assert.deepEqual(r.calls.filter((c) => c.command === "register_extension").map((c) => c.args), [{ browser: "edge" }]);
  const failing = rig({ failRegister: true });
  await failing.settle();
  [...failing.page.element.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Register")!.click();
  await failing.settle();
  assert.match(failing.page.element.textContent ?? "", /Could not register this browser\. Nothing was changed\. Try again\./);
});

test("the shell mounts the page on Connections only, and destroys it on leaving, locking and destroy", async () => {
  const dom = new JSDOM('<!doctype html><body><div id="root"></div></body>', { pretendToBeVisual: true, url: "http://localhost/" });
  const doc = dom.window.document;
  Object.assign(globalThis, { document: doc, window: dom.window });
  const invoke: Invoke = async <T>(command: string) => {
    if (command === "get_local_mcp_status") return { running: false } as T;
    if (command === "list_local_mcp_clients") return [] as T;
    if (command === "get_setup") return { registered_browsers: [] } as T;
    return undefined as T;
  };
  const i18n = createI18nController({ initialLocale: "en", storage: { getItem: () => null, setItem: () => {} } });
  const shell = mountUi2Shell(doc.getElementById("root")!, { onLock: () => {}, i18n, pages: createUi2Pages({ invoke }) });
  assert.match(doc.body.textContent ?? "", /This section is not ready/, "other destinations keep the placeholder");
  shell.navigate("/connections");
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.match(doc.body.textContent ?? "", /Let an AI app read your memory/);
  assert.doesNotMatch(doc.body.textContent ?? "", /This section is not ready/);
  shell.navigate("/memory");
  assert.doesNotMatch(doc.body.textContent ?? "", /Let an AI app read your memory/, "leaving destroys the page");
  shell.navigate("/connections");
  shell.destroy();
  assert.equal(doc.body.textContent?.includes("Let an AI app read your memory"), false);
});

function hostedControls(r: ReturnType<typeof rig>) {
  const details = [...r.page.element.querySelectorAll<HTMLDetailsElement>("details")].find((d) =>
    (d.textContent ?? "").includes("Advanced: connect hosted ChatGPT"),
  )!;
  const inputs = details.querySelectorAll<HTMLInputElement>("input");
  return {
    details,
    id: inputs[0],
    key: inputs[1],
    consent: details.querySelector<HTMLButtonElement>("[role=switch]")!,
    start: [...details.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Start connection")!,
    stop: [...details.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Stop and revoke access")!,
  };
}

test("hosted connection is collapsed, needs explicit consent, clears the key when sent, and never claims connected", async () => {
  const r = rig();
  await r.settle();
  let c = hostedControls(r);
  assert.equal(c.details.open, false, "Advanced is collapsed by default");
  assert.equal(c.key.type, "password");
  assert.equal(c.start.disabled, true);
  c.id.value = "tunnel_abc";
  c.id.dispatchEvent(new r.dom.window.Event("input", { bubbles: true }));
  c.key.value = "sk-private-runtime-key";
  c.key.dispatchEvent(new r.dom.window.Event("input", { bubbles: true }));
  assert.equal(c.start.disabled, true, "id and key alone are not consent");
  c.consent.click();
  assert.equal(c.start.disabled, false);
  c.start.click();
  await r.settle();
  const started = r.calls.find((call) => call.command === "start_hosted_tunnel")!;
  assert.deepEqual(started.args, { tunnelId: "tunnel_abc", apiKey: "sk-private-runtime-key", acknowledgedRemoteAccess: true });
  c = hostedControls(r);
  assert.equal(c.key.value, "", "the key is cleared from the field once sent");
  assert.equal((r.doc.body.innerHTML).includes("sk-private-runtime-key"), false, "the key is not left in the page");
  const text = r.page.element.textContent ?? "";
  assert.match(text, /The connection helper is running\. It has not been checked that ChatGPT is connected\./);
  assert.doesNotMatch(text, /Connected|tunnel_secret_id/, "no connected claim and no tunnel identifier in the default view");
  c.stop.click();
  await r.settle();
  assert.ok(r.calls.some((call) => call.command === "stop_hosted_tunnel"));
  assert.match(r.page.element.textContent ?? "", /The hosted connection is off\./);
});

test("hosted start errors are plain, and a damaged helper gets repair guidance; destroy drops secrets", async () => {
  const bad = rig({ tunnelError: "MP_TUNNEL_INTEGRITY:damaged" });
  await bad.settle();
  const c = hostedControls(bad);
  c.id.value = "tunnel_abc";
  c.id.dispatchEvent(new bad.dom.window.Event("input", { bubbles: true }));
  c.key.value = "sk-another-private-key";
  c.key.dispatchEvent(new bad.dom.window.Event("input", { bubbles: true }));
  c.consent.click();
  c.start.click();
  await bad.settle();
  assert.match(bad.page.element.textContent ?? "", /bundled connection helper is missing or damaged/);
  assert.equal(bad.doc.body.innerHTML.includes("sk-another-private-key"), false);
  const generic = rig({ tunnelError: "MP_X" });
  await generic.settle();
  const g = hostedControls(generic);
  g.id.value = "tunnel_abc";
  g.id.dispatchEvent(new generic.dom.window.Event("input", { bubbles: true }));
  g.key.value = "k";
  g.key.dispatchEvent(new generic.dom.window.Event("input", { bubbles: true }));
  g.consent.click();
  g.start.click();
  await generic.settle();
  assert.match(generic.page.element.textContent ?? "", /Could not start\. Check the tunnel ID and key, then try again\./);
  generic.page.destroy();
  assert.equal(generic.page.element.childElementCount, 0);
});

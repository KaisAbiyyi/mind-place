import assert from "node:assert/strict";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import {
  ONBOARDING_STEPS, createMemoryProgressStore, describeCapture, describeMcp, describeVault, hasFinishedOnboarding,
  isLockedFailure, mountOnboardingWorkspace, parseOnboardingProgress,
  type OnboardingHooks, type OnboardingProgress, type OnboardingWorkspaceHandle,
} from "../src/onboarding-workspace.js";
import type { Invoke } from "../src/reader.js";

const tick = async () => { await new Promise((resolve) => setTimeout(resolve, 0)); };
const settle = async () => { for (let i = 0; i < 6; i++) await tick(); };

const secure = { configured: true, locked: false, epoch: 1, retry_after_ms: 0, vault_mode: "encrypted" };
const setupInfo = {
  archive_path: "C:/archive", extension_path: "C:/ext", host_path: "C:/host.exe", extension_id: "abc",
  native_host_name: "com.mindplace.capture", registered_browsers: [] as string[], platform: "windows", legacy_available: false,
};
const summary = (id: string, title: string, capturedAt: string) => ({
  id, conversation_id: id, provider: "chatgpt", title, source_url: "https://chatgpt.com/c/" + id, captured_at: capturedAt,
  message_count: 2, snapshot_count: 1, completeness: "complete", warnings: [],
});
const conversation = (id: string, snapshotId = "snap-1", messages = [{ id: "m1", role: "user", text: "Hello archive" }]) => ({
  summary: summary(id, "Plan " + id, "2026-11-01T00:00:00Z"), snapshot_id: snapshotId,
  capture: { title: "Plan " + id, captured_at: "2026-11-01T00:00:00Z", messages },
});

interface Env { dom: JSDOM; view: HTMLElement; hooks: OnboardingHooks; calls: string[]; store: ReturnType<typeof createMemoryProgressStore>; mount(): OnboardingWorkspaceHandle; }
function environment(invoke: Invoke, initial: OnboardingProgress | null = null): Env {
  const dom = new JSDOM("<!doctype html><main id='view'></main>", { url: "https://app.test/" });
  const view = dom.window.document.querySelector<HTMLElement>("#view")!;
  const calls: string[] = [];
  const store = createMemoryProgressStore(initial);
  const hooks: OnboardingHooks = {
    openRoute: (route) => { calls.push("route:" + route); },
    openSetupDetails: () => { calls.push("details"); },
    openConversation: (id, snapshotId) => { calls.push("open:" + id + ":" + (snapshotId ?? "")); },
    onFinish: () => { calls.push("finish"); },
  };
  return { dom, view, hooks, calls, store, mount: () => mountOnboardingWorkspace({ view, invoke, hooks, store }) };
}
const click = (view: HTMLElement, selector: string) => {
  const target = view.querySelector<HTMLElement>(selector);
  assert.ok(target, "missing " + selector);
  target.click();
};
const text = (view: HTMLElement) => view.textContent ?? "";

test("welcome step is static, explains required vs optional and loads no backend state", async () => {
  const seen: string[] = [];
  const invoke: Invoke = async <T>(name: string) => { seen.push(name); throw new Error(name); };
  const env = environment(invoke);
  const handle = env.mount();
  await handle.ready;
  assert.deepEqual(seen, []);
  assert.match(text(env.view), /Required — your unlock secret/);
  assert.match(text(env.view), /Optional — browser capture/);
  assert.equal(env.view.querySelector("[aria-current='step']")?.getAttribute("data-step"), "welcome");
  assert.equal(handle.state().status, "ready");
  handle.destroy();
});

test("vault status reflects real security state and never fabricates encryption", async () => {
  let status: Record<string, unknown> = { ...secure, vault_mode: "legacy" };
  const invoke: Invoke = async <T>(name: string) => {
    if (name === "get_security_status") return status as T;
    throw new Error(name);
  };
  const env = environment(invoke);
  const handle = env.mount();
  handle.goTo("vault");
  await settle();
  assert.match(text(env.view), /Unlock secret only/);
  assert.match(text(env.view), /not encrypted at rest/);
  assert.doesNotMatch(text(env.view), /Encrypted vault unlocked/);
  status = { ...secure, vault_mode: "encrypted" };
  click(env.view, "[data-focus-key='refresh-vault']");
  await settle();
  assert.match(text(env.view), /Encrypted vault unlocked/);
  assert.equal(handle.state().vault, "unlocked");
  assert.match(text(env.view), /offline recovery code/);
  assert.doesNotMatch(env.view.innerHTML, /<input[^>]+type="password"/);
  handle.destroy();
});

test("locked and failed vault status show recovery paths instead of success", async () => {
  let mode: "locked" | "fail" = "locked";
  const invoke: Invoke = async <T>(name: string) => {
    if (name !== "get_security_status") throw new Error(name);
    if (mode === "fail") throw new Error("disk read failure with /secret/path");
    return { ...secure, locked: true } as T;
  };
  const env = environment(invoke);
  const handle = env.mount();
  handle.goTo("vault");
  await settle();
  assert.equal(handle.state().status, "locked");
  assert.match(text(env.view), /Locked/);
  mode = "fail";
  click(env.view, "[data-focus-key='refresh-vault']");
  await settle();
  assert.equal(handle.state().status, "error");
  assert.match(text(env.view), /Vault status is unavailable/);
  assert.doesNotMatch(text(env.view), /secret\/path/);
  handle.destroy();
});

test("providers step: permission explanation, untested registration, optional skip with resume", async () => {
  const registered: string[] = [];
  const invoke: Invoke = async <T>(name: string, args?: Record<string, unknown>) => {
    if (name === "get_setup") return { ...setupInfo, registered_browsers: [...registered] } as T;
    if (name === "get_local_mcp_status") return { running: true, endpoint: "http://127.0.0.1:43731/mcp" } as T;
    if (name === "register_extension") { registered.push(String(args?.browser)); return { ...setupInfo, registered_browsers: [...registered] } as T; }
    throw new Error(name);
  };
  const env = environment(invoke);
  const handle = env.mount();
  handle.goTo("providers");
  await settle();
  assert.match(text(env.view), /activeTab \+ scripting/);
  assert.match(text(env.view), /Automatic saving is off/);
  assert.match(text(env.view), /Listener running \(no client verified\)/);
  assert.match(text(env.view), /Not registered/);
  click(env.view, "[data-focus-key='register-edge']");
  await settle();
  assert.deepEqual(registered, ["edge"]);
  assert.match(text(env.view), /connection has not been tested/);
  assert.match(text(env.view), /1 browser registered \(connection untested\)/);
  assert.doesNotMatch(text(env.view), /Connected|Authorized/);
  click(env.view, "[data-focus-key='open-providers']");
  click(env.view, "[data-focus-key='open-files']");
  click(env.view, "[data-focus-key='open-details']");
  assert.deepEqual(env.calls, ["route:ai-providers-mcp", "route:files-secure-vault", "details"]);
  click(env.view, "[data-focus-key='skip-providers']");
  await settle();
  assert.equal(env.view.querySelector("[aria-current='step']")?.getAttribute("data-step"), "first-result");
  assert.deepEqual(env.store.read(), { version: 1, step: "first-result", providersSkipped: true, finishedAt: null });
  handle.destroy();
  // Resume: a new mount returns to the saved step and remembers the skip.
  const resumed = environment(invoke, env.store.read());
  const again = resumed.mount();
  assert.equal(resumed.view.querySelector("[aria-current='step']")?.getAttribute("data-step"), "first-result");
  click(resumed.view, "[data-step='providers']");
  await settle();
  assert.ok(resumed.view.querySelector("[data-skipped='true']"));
  again.destroy();
});

test("registration failure keeps the previous state and explains recovery without leaking details", async () => {
  const invoke: Invoke = async <T>(name: string) => {
    if (name === "get_setup") return setupInfo as T;
    if (name === "get_local_mcp_status") return { running: false } as T;
    if (name === "register_extension") throw new Error("registry denied: HKCU\\Software\\private");
    throw new Error(name);
  };
  const env = environment(invoke);
  const handle = env.mount();
  handle.goTo("providers");
  await settle();
  click(env.view, "[data-focus-key='register-chrome']");
  await settle();
  const alert = env.view.querySelector("[role='alert']");
  assert.ok(alert);
  assert.match(alert.textContent!, /Couldn’t register Chrome/);
  assert.doesNotMatch(text(env.view), /HKCU|private/);
  assert.match(text(env.view), /Not registered/);
  assert.equal(env.view.querySelector<HTMLButtonElement>("[data-focus-key='register-chrome']")!.disabled, false);
  handle.destroy();
});

test("first result: empty archive is an honest empty state with import/capture recovery", async () => {
  const invoke: Invoke = async <T>(name: string) => {
    if (name === "list_conversations") return [] as T;
    throw new Error(name);
  };
  const env = environment(invoke);
  const handle = env.mount();
  handle.goTo("first-result");
  await settle();
  assert.match(text(env.view), /No saved conversations yet/);
  assert.equal(env.view.querySelector("[data-verified='true']"), null);
  assert.match(text(env.view), /does not mean a result was verified/);
  click(env.view, "[data-focus-key='open-details']");
  assert.deepEqual(env.calls, ["details"]);
  handle.destroy();
});

test("first result: latest read and search both require a real, identity-matched read", async () => {
  const reads: unknown[] = [];
  const invoke: Invoke = async <T>(name: string, args?: Record<string, unknown>) => {
    if (name === "list_conversations") return [summary("a", "Old", "2026-10-01T00:00:00Z"), summary("b", "Newest", "2026-11-02T00:00:00Z")] as T;
    if (name === "read_conversation") { reads.push(args); return conversation(String(args?.id), String(args?.snapshotId ?? "snap-latest")) as T; }
    if (name === "search_archive_index") {
      assert.equal(args?.limit, 5);
      return [{ source_id: "s1", archive_id: "a", snapshot_id: "snap-7", message_id: "m1", kind: "message", role: "user", title: "Old", snippet: "Hello archive", score: 1 }] as T;
    }
    throw new Error(name);
  };
  const env = environment(invoke);
  const handle = env.mount();
  handle.goTo("first-result");
  await settle();
  assert.match(text(env.view), /2 saved conversations/);
  assert.match(text(env.view), /Read latest conversation \(Newest\)/);
  click(env.view, "[data-focus-key='read-latest']");
  await settle();
  assert.deepEqual(reads[0], { id: "b" });
  assert.ok(env.view.querySelector("[data-verified='true']"));
  assert.match(text(env.view), /snap-latest/);
  assert.equal(handle.state().data && (handle.state().data as { firstResultVerified: boolean }).firstResultVerified, true);
  const input = env.view.querySelector<HTMLInputElement>("#onboarding-query")!;
  input.value = "archive";
  input.dispatchEvent(new env.dom.window.Event("input", { bubbles: true }));
  env.view.querySelector("form")!.dispatchEvent(new env.dom.window.Event("submit", { bubbles: true, cancelable: true }));
  await settle();
  assert.equal(env.view.querySelector("[data-verified='true']"), null, "a new search clears the previous verification");
  assert.match(text(env.view), /1 match found/);
  click(env.view, ".onboarding-hit");
  await settle();
  assert.deepEqual(reads[1], { id: "a", snapshotId: "snap-7" });
  assert.match(text(env.view), /Found by\s*Search/);
  click(env.view, "[data-focus-key='open-conversation']");
  assert.deepEqual(env.calls, ["open:a:snap-7"]);
  click(env.view, "[data-focus-key='finish']");
  assert.deepEqual(env.calls.slice(-1), ["finish"]);
  assert.ok(hasFinishedOnboarding(env.store));
  handle.destroy();
});

test("first result: mismatched identity, empty transcript and read failures never count as success", async () => {
  let mode: "mismatch" | "empty" | "fail" | "locked" = "mismatch";
  const invoke: Invoke = async <T>(name: string, args?: Record<string, unknown>) => {
    if (name === "list_conversations") return [summary("a", "Only", "2026-11-02T00:00:00Z")] as T;
    if (name === "read_conversation") {
      if (mode === "mismatch") return conversation("different") as T;
      if (mode === "empty") return conversation(String(args?.id), "snap", []) as T;
      if (mode === "locked") throw new Error("MP_SECURITY_LOCKED");
      throw new Error("C:/Users/private/archive.json missing");
    }
    throw new Error(name);
  };
  const env = environment(invoke);
  const handle = env.mount();
  handle.goTo("first-result");
  await settle();
  for (const expected of [/Couldn’t read that conversation/, /no readable messages/, /Couldn’t read that conversation/, /Mind Place is locked/] as const) {
    click(env.view, "[data-focus-key='read-latest']");
    await settle();
    assert.equal(env.view.querySelector("[data-verified='true']"), null);
    assert.match(text(env.view), expected);
    assert.doesNotMatch(text(env.view), /private|archive\.json/);
    mode = mode === "mismatch" ? "empty" : mode === "empty" ? "fail" : "locked";
  }
  handle.destroy();
});

test("search failure and lock produce recoverable states; bad queries are rejected before invoking", async () => {
  let searches = 0;
  const invoke: Invoke = async <T>(name: string) => {
    if (name === "list_conversations") return [summary("a", "Only", "2026-11-02T00:00:00Z")] as T;
    if (name === "search_archive_index") { searches++; throw new Error("index corrupt /private"); }
    throw new Error(name);
  };
  const env = environment(invoke);
  const handle = env.mount();
  handle.goTo("first-result");
  await settle();
  env.view.querySelector("form")!.dispatchEvent(new env.dom.window.Event("submit", { bubbles: true, cancelable: true }));
  await settle();
  assert.equal(searches, 0);
  assert.match(text(env.view), /Enter a search phrase/);
  const input = env.view.querySelector<HTMLInputElement>("#onboarding-query")!;
  input.value = "x";
  input.dispatchEvent(new env.dom.window.Event("input", { bubbles: true }));
  env.view.querySelector("form")!.dispatchEvent(new env.dom.window.Event("submit", { bubbles: true, cancelable: true }));
  await settle();
  assert.equal(searches, 1);
  assert.match(text(env.view), /Search is unavailable/);
  assert.doesNotMatch(text(env.view), /private/);
  handle.destroy();
});

test("archive list failure while locked asks the owner to unlock and can be rechecked", async () => {
  let locked = true;
  const invoke: Invoke = async <T>(name: string) => {
    if (name !== "list_conversations") throw new Error(name);
    if (locked) throw new Error("MP_SECURITY_LOCKED");
    return [summary("a", "Back", "2026-11-02T00:00:00Z")] as T;
  };
  const env = environment(invoke);
  const handle = env.mount();
  handle.goTo("first-result");
  await settle();
  assert.equal(handle.state().status, "locked");
  assert.match(text(env.view), /Unlock Mind Place from the lock screen/);
  locked = false;
  click(env.view, "[data-focus-key='refresh-archive']");
  await settle();
  assert.match(text(env.view), /1 saved conversation/);
  handle.destroy();
});

test("destroy clears sensitive content and ignores late responses", async () => {
  let release!: (value: unknown) => void;
  const pending = new Promise((resolve) => { release = resolve; });
  const invoke: Invoke = async <T>(name: string) => {
    if (name === "list_conversations") return [summary("a", "Private title", "2026-11-02T00:00:00Z")] as T;
    if (name === "read_conversation") return await pending as T;
    throw new Error(name);
  };
  const env = environment(invoke);
  const handle = env.mount();
  handle.goTo("first-result");
  await settle();
  click(env.view, "[data-focus-key='read-latest']");
  await tick();
  handle.destroy();
  assert.equal(env.view.textContent, "");
  release(conversation("a"));
  await settle();
  assert.equal(env.view.textContent, "");
  assert.equal(env.view.querySelector("[data-verified]"), null);
});

test("stale step data cannot overwrite a newer step", async () => {
  let releaseSetup!: (value: unknown) => void;
  const slow = new Promise((resolve) => { releaseSetup = resolve; });
  const invoke: Invoke = async <T>(name: string) => {
    if (name === "get_setup") return await slow as T;
    if (name === "get_local_mcp_status") return { running: false } as T;
    if (name === "get_security_status") return secure as T;
    throw new Error(name);
  };
  const env = environment(invoke);
  const handle = env.mount();
  handle.goTo("providers");
  await tick();
  handle.goTo("vault");
  await settle();
  releaseSetup({ ...setupInfo, registered_browsers: ["chrome"] });
  await settle();
  assert.match(text(env.view), /Check your secure vault/);
  assert.doesNotMatch(text(env.view), /browser registered/);
  handle.destroy();
});

test("stepper is keyboard reachable, marks the current step and moves focus to the new heading", async () => {
  const invoke: Invoke = async <T>(name: string) => {
    if (name === "get_security_status") return secure as T;
    throw new Error(name);
  };
  const env = environment(invoke);
  const handle = env.mount();
  const buttons = [...env.view.querySelectorAll<HTMLButtonElement>(".onboarding-step-button")];
  assert.equal(buttons.length, ONBOARDING_STEPS.length);
  assert.ok(buttons.every((button) => button.tagName === "BUTTON" && button.type === "button"));
  assert.equal(env.view.querySelector(".onboarding-counter")!.textContent, "Step 1 of 4");
  assert.match(text(env.view.querySelector(".onboarding-steps")! as HTMLElement), /Required/);
  assert.match(text(env.view.querySelector(".onboarding-steps")! as HTMLElement), /Optional/);
  click(env.view, "[data-focus-key='next']");
  await settle();
  assert.equal(env.view.querySelector(".onboarding-counter")!.textContent, "Step 2 of 4");
  assert.equal(env.dom.window.document.activeElement?.tagName, "H2");
  handle.destroy();
});

test("progress storage is validated, non-sensitive and finished wizards do not auto-resume", async () => {
  assert.equal(parseOnboardingProgress({ version: 1, step: "nope", providersSkipped: false, finishedAt: null }), null);
  assert.equal(parseOnboardingProgress({ version: 2, step: "vault", providersSkipped: false, finishedAt: null }), null);
  assert.equal(parseOnboardingProgress({ version: 1, step: "vault", providersSkipped: false, finishedAt: "yesterday" }), null);
  assert.equal(parseOnboardingProgress("vault"), null);
  const ok = parseOnboardingProgress({ version: 1, step: "vault", providersSkipped: true, finishedAt: null, secret: "must-not-survive" });
  assert.deepEqual(ok, { version: 1, step: "vault", providersSkipped: true, finishedAt: null });
  const invoke: Invoke = async <T>(name: string) => { throw new Error(name); };
  const env = environment(invoke, { version: 1, step: "first-result", providersSkipped: false, finishedAt: "2026-11-01T00:00:00.000Z" });
  const handle = env.mount();
  assert.equal(env.view.querySelector("[aria-current='step']")?.getAttribute("data-step"), "welcome");
  assert.match(text(env.view), /You finished setup earlier/);
  assert.ok(hasFinishedOnboarding(env.store));
  handle.destroy();
});

test("pure status helpers never claim more than the backend reported", () => {
  assert.equal(describeVault({ status: "pending" }).state, "loading");
  assert.equal(describeVault({ status: "ready", value: { ...secure, locked: true } as never }).state, "locked");
  assert.equal(describeVault({ status: "ready", value: { ...secure, configured: false } as never }).state, "partial");
  assert.equal(describeVault({ status: "ready", value: { ...secure, vault_mode: undefined } as never }).state, "partial");
  assert.equal(describeVault({ status: "ready", value: secure as never }).state, "complete");
  assert.equal(describeCapture({ status: "ready", value: { ...setupInfo, registered_browsers: ["chrome", "edge"] } as never }).state, "partial");
  assert.equal(describeMcp({ status: "ready", value: { running: true } }).state, "partial");
  assert.equal(describeMcp({ status: "ready", value: { running: false } }).state, "neutral");
  assert.equal(isLockedFailure(new Error("MP_UI_SESSION_EXPIRED")), true);
  assert.equal(isLockedFailure(new Error("file missing")), false);
});

test("caller-controlled text is rendered inertly", async () => {
  const hostile = "<img src=x onerror=alert(1)><script>window.pwned=1</script>";
  const invoke: Invoke = async <T>(name: string, args?: Record<string, unknown>) => {
    if (name === "list_conversations") return [summary("a", hostile, "2026-11-02T00:00:00Z")] as T;
    if (name === "read_conversation") return conversation(String(args?.id), "snap", [{ id: "m1", role: "user", text: hostile }]) as T;
    throw new Error(name);
  };
  const env = environment(invoke);
  const handle = env.mount();
  handle.goTo("first-result");
  await settle();
  click(env.view, "[data-focus-key='read-latest']");
  await settle();
  assert.ok(env.view.querySelector("[data-verified='true']"));
  assert.equal(env.view.querySelector("img"), null);
  assert.equal(env.view.querySelector("script"), null);
  assert.match(text(env.view), /<img src=x/);
  handle.destroy();
});

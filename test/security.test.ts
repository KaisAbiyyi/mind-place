import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";
import { JSDOM } from "jsdom";
import { test } from "node:test";
import { mountSecurityGate, type SecurityStatus } from "../src/security.js";
import type { Invoke } from "../src/reader.js";

function domRoot() {
  const dom = new JSDOM("<!doctype html><html><body><div id='app'></div></body></html>", {
    url: "https://security.test/",
  });
  return { dom, root: dom.window.document.querySelector<HTMLElement>("#app")! };
}

const lockedUnconfigured: SecurityStatus = {
  configured: false,
  locked: true,
  epoch: 0,
  retry_after_ms: 0,
};

const lockedConfigured: SecurityStatus = {
  configured: true,
  locked: true,
  epoch: 0,
  retry_after_ms: 0,
};

test("first run confirms and creates a secret before opening the reader", async () => {
  const { dom, root } = domRoot();
  const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
  let opened = 0;
  const invoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
    calls.push({ command, args });
    if (command === "get_security_status") return lockedUnconfigured as T;
    if (command === "initialize_security") {
      return { configured: true, locked: false, epoch: 1, retry_after_ms: 0 } as T;
    }
    throw new Error(`unexpected ${command}`);
  };

  await mountSecurityGate({ root, invoke, onUnlocked: () => { opened += 1; } });
  assert.match(root.textContent ?? "", /Protect Mind Place/);
  assert.match(root.textContent ?? "", /does not yet claim to encrypt existing raw archive files/i);

  const input = root.querySelector<HTMLInputElement>("#security-secret")!;
  const confirm = root.querySelector<HTMLInputElement>("#security-secret-confirm")!;
  input.value = "correct horse battery staple";
  confirm.value = "correct horse battery staple";
  root.querySelector<HTMLFormElement>("#security-form")!.dispatchEvent(new dom.window.Event("submit", { bubbles: true, cancelable: true }));
  await delay(0);

  assert.equal(opened, 1);
  assert.equal(input.value, "");
  assert.equal(confirm.value, "");
  assert.deepEqual(calls.at(-1), {
    command: "initialize_security",
    args: { secret: "correct horse battery staple" },
  });
});

test("first run refuses mismatched confirmation without invoking initialization", async () => {
  const { dom, root } = domRoot();
  const calls: string[] = [];
  const invoke: Invoke = async <T>(command: string) => {
    calls.push(command);
    if (command === "get_security_status") return lockedUnconfigured as T;
    throw new Error(`unexpected ${command}`);
  };

  await mountSecurityGate({ root, invoke, onUnlocked: () => assert.fail("must remain locked") });
  const input = root.querySelector<HTMLInputElement>("#security-secret")!;
  const confirm = root.querySelector<HTMLInputElement>("#security-secret-confirm")!;
  input.value = "correct horse battery staple";
  confirm.value = "correct horse battery stapler";
  root.querySelector<HTMLFormElement>("#security-form")!.dispatchEvent(new dom.window.Event("submit", { bubbles: true, cancelable: true }));
  await delay(0);

  assert.deepEqual(calls, ["get_security_status"]);
  assert.equal(input.value, "");
  assert.equal(confirm.value, "");
  assert.match(root.querySelector("#security-error")?.textContent ?? "", /confirmation does not match/i);
});

test("configured startup stays locked after an invalid secret", async () => {
  const { dom, root } = domRoot();
  let opened = 0;
  const invoke: Invoke = async <T>(command: string) => {
    if (command === "get_security_status") return lockedConfigured as T;
    if (command === "unlock_security") throw new Error("MP_SECURITY_INVALID_CREDENTIALS:Unlock secret is incorrect");
    throw new Error(`unexpected ${command}`);
  };

  await mountSecurityGate({ root, invoke, onUnlocked: () => { opened += 1; } });
  const input = root.querySelector<HTMLInputElement>("#security-secret")!;
  input.value = "wrong password";
  root.querySelector<HTMLFormElement>("#security-form")!.dispatchEvent(new dom.window.Event("submit", { bubbles: true, cancelable: true }));
  await delay(0);

  assert.equal(opened, 0);
  assert.equal(input.value, "");
  assert.match(root.querySelector("#security-error")?.textContent ?? "", /Incorrect PIN or password/);
  assert.equal(input.disabled, false);
});

test("already-unlocked runtime skips the gate", async () => {
  const { root } = domRoot();
  let opened = 0;
  const invoke: Invoke = async <T>(command: string) => {
    assert.equal(command, "get_security_status");
    return { configured: true, locked: false, epoch: 9, retry_after_ms: 0 } as T;
  };

  await mountSecurityGate({ root, invoke, onUnlocked: () => { opened += 1; } });
  assert.equal(opened, 1);
  assert.equal(root.innerHTML, "");
});

test("new installation stages recovery before any legacy runtime unlock", async () => {
  const { dom, root } = domRoot();
  const calls: string[] = [];
  let opened = 0;
  const code = "MP-RECOVERY-1-synthetic-secret.checksum";
  const invoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
    calls.push(command);
    if (command === "get_security_status") return lockedUnconfigured as T;
    if (command === "initialize_security") throw new Error("MP_VAULT_PROVISION_REQUIRED");
    if (command === "begin_vault_setup") {
      assert.equal(args?.secret, "correct horse battery staple");
      return code as T;
    }
    if (command === "confirm_vault_setup") {
      assert.equal(args?.code, code);
      return undefined as T;
    }
    throw new Error(`unexpected ${command}`);
  };
  await mountSecurityGate({ root, invoke, onUnlocked: () => { opened += 1; } });
  root.querySelector<HTMLInputElement>("#security-secret")!.value = "correct horse battery staple";
  root.querySelector<HTMLInputElement>("#security-secret-confirm")!.value = "correct horse battery staple";
  root.querySelector<HTMLFormElement>("#security-form")!.dispatchEvent(
    new dom.window.Event("submit", { bubbles: true, cancelable: true }),
  );
  await delay(0);
  assert.equal(opened, 0);
  assert.equal(root.querySelector("#vault-code")?.textContent, code);
  const entered = root.querySelector<HTMLInputElement>("#vault-confirm-code")!;
  entered.value = code;
  root.querySelector<HTMLInputElement>("#vault-saved")!.checked = true;
  root.querySelector<HTMLFormElement>("#vault-confirm-form")!.dispatchEvent(
    new dom.window.Event("submit", { bubbles: true, cancelable: true }),
  );
  await delay(0);
  assert.equal(opened, 0);
  assert.equal(root.querySelector("#vault-code"), null);
  assert.match(root.textContent ?? "", /remains locked until the encrypted runtime/i);
  assert.deepEqual(calls, [
    "get_security_status", "initialize_security", "begin_vault_setup", "confirm_vault_setup",
  ]);
});

test("wrong recovery code leaves the ceremony open without unlocking", async () => {
  const { dom, root } = domRoot();
  let opened = false;
  const invoke: Invoke = async <T>(command: string) => {
    if (command === "get_security_status") return lockedUnconfigured as T;
    if (command === "initialize_security") throw new Error("MP_VAULT_PROVISION_REQUIRED");
    if (command === "begin_vault_setup") return "synthetic-code" as T;
    if (command === "confirm_vault_setup") throw new Error("wrong key");
    throw new Error(`unexpected ${command}`);
  };
  await mountSecurityGate({ root, invoke, onUnlocked: () => { opened = true; } });
  root.querySelector<HTMLInputElement>("#security-secret")!.value = "correct horse battery staple";
  root.querySelector<HTMLInputElement>("#security-secret-confirm")!.value = "correct horse battery staple";
  root.querySelector<HTMLFormElement>("#security-form")!.dispatchEvent(
    new dom.window.Event("submit", { bubbles: true, cancelable: true }),
  );
  await delay(0);
  const entered = root.querySelector<HTMLInputElement>("#vault-confirm-code")!;
  entered.value = "incorrect";
  root.querySelector<HTMLInputElement>("#vault-saved")!.checked = true;
  root.querySelector<HTMLFormElement>("#vault-confirm-form")!.dispatchEvent(
    new dom.window.Event("submit", { bubbles: true, cancelable: true }),
  );
  await delay(0);
  assert.equal(opened, false);
  assert.equal(entered.value, "");
  assert.match(root.querySelector("#vault-error")?.textContent ?? "", /confirmation failed/i);
});

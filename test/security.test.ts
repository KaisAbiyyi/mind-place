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
  let provisioned = false;
  const code = "MP-RECOVERY-1-synthetic-secret.checksum";
  const invoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
    calls.push(command);
    if (command === "get_security_status") {
      return (provisioned ? { ...lockedConfigured, vault_mode: "encrypted" } : lockedUnconfigured) as T;
    }
    if (command === "unlock_security") {
      assert.equal(args?.secret, "correct horse battery staple");
      return { ...lockedConfigured, locked: false, epoch: 2, vault_mode: "encrypted" } as T;
    }
    if (command === "initialize_security") throw new Error("MP_VAULT_PROVISION_REQUIRED");
    if (command === "begin_vault_setup") {
      assert.equal(args?.secret, "correct horse battery staple");
      return code as T;
    }
    if (command === "confirm_vault_setup") {
      assert.equal(args?.code, code);
      provisioned = true;
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
  assert.ok(root.querySelector("#security-form"), "confirmation must return to explicit unlock");
  assert.ok(root.querySelector("#security-secret"));
  assert.deepEqual(calls, [
    "get_security_status", "initialize_security", "begin_vault_setup", "confirm_vault_setup", "get_security_status",
  ]);
  root.querySelector<HTMLInputElement>("#security-secret")!.value = "correct horse battery staple";
  root.querySelector<HTMLFormElement>("#security-form")!.dispatchEvent(
    new dom.window.Event("submit", { bubbles: true, cancelable: true }),
  );
  await delay(0);
  assert.equal(opened, 1);
  assert.equal(calls.at(-1), "unlock_security");
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

test("already-unlocked encrypted verifier resumes recovery, never the legacy reader", async () => {
  const { root } = domRoot();
  let opened = 0;
  const calls: string[] = [];
  const invoke: Invoke = async <T>(command: string) => {
    calls.push(command);
    if (command === "get_security_status") {
      return { configured: true, locked: false, epoch: 7, retry_after_ms: 0, vault_mode: "encrypted" } as T;
    }
    if (command === "resume_vault_setup") return "synthetic-offline-code" as T;
    throw new Error(`unexpected ${command}`);
  };
  await mountSecurityGate({ root, invoke, onUnlocked: () => { opened += 1; } });
  assert.deepEqual(calls, ["get_security_status", "resume_vault_setup"]);
  assert.equal(opened, 0);
  assert.equal(root.querySelector("#vault-code")?.textContent, "synthetic-offline-code");
});


test("aborted first-run restart requires authenticated fresh recovery confirmation", async () => {
  const secret = "synthetic-first-run-password";
  const calls: string[] = [];
  let configured = false;
  let published = false;
  let pendingCode: string | null = null;
  let ceremony = 0;
  let opened = 0;
  const invoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
    calls.push(command);
    if (command === "get_security_status") {
      return { ...lockedConfigured, configured, vault_mode: "encrypted" } as T;
    }
    if (command === "initialize_security") throw new Error("MP_VAULT_PROVISION_REQUIRED");
    if (command === "unlock_security") {
      if (args?.secret !== secret) throw new Error("MP_SECURITY_INVALID_CREDENTIALS");
      if (!published) throw new Error("MP_VAULT_INCOMPLETE_FIRST_RUN");
      return { ...lockedConfigured, locked: false, vault_mode: "encrypted" } as T;
    }
    if (command === "begin_vault_setup") {
      assert.equal(args?.secret, secret);
      assert.equal(published, false);
      configured = true;
      pendingCode = "synthetic-code-" + ++ceremony;
      return pendingCode as T;
    }
    if (command === "confirm_vault_setup") {
      if (args?.code !== pendingCode) throw new Error("MP_RECOVERY_MISMATCH");
      published = true;
      pendingCode = null;
      return undefined as T;
    }
    throw new Error("unexpected command: " + command);
  };
  const first = domRoot();
  await mountSecurityGate({ root: first.root, invoke, onUnlocked: () => { opened++; } });
  first.root.querySelector<HTMLInputElement>("#security-secret")!.value = secret;
  first.root.querySelector<HTMLInputElement>("#security-secret-confirm")!.value = secret;
  first.root.querySelector<HTMLFormElement>("#security-form")!.dispatchEvent(
    new first.dom.window.Event("submit", { bubbles: true, cancelable: true }),
  );
  await delay(0);
  assert.equal(first.root.querySelector("#vault-code")?.textContent, "synthetic-code-1");
  assert.equal(opened, 0);

  // Process and recovery proposal were lost, but the verifier remains.
  pendingCode = null;
  const restarted = domRoot();
  await mountSecurityGate({ root: restarted.root, invoke, onUnlocked: () => { opened++; } });
  const input = restarted.root.querySelector<HTMLInputElement>("#security-secret")!;
  input.value = "wrong synthetic password";
  restarted.root.querySelector<HTMLFormElement>("#security-form")!.dispatchEvent(
    new restarted.dom.window.Event("submit", { bubbles: true, cancelable: true }),
  );
  await delay(0);
  assert.match(restarted.root.querySelector("#security-error")?.textContent ?? "", /Incorrect/);
  assert.equal(ceremony, 1);
  input.value = secret;
  restarted.root.querySelector<HTMLFormElement>("#security-form")!.dispatchEvent(
    new restarted.dom.window.Event("submit", { bubbles: true, cancelable: true }),
  );
  await delay(0);
  assert.equal(restarted.root.querySelector("#vault-code")?.textContent, "synthetic-code-2");
  assert.equal(opened, 0);
  const entered = restarted.root.querySelector<HTMLInputElement>("#vault-confirm-code")!;
  restarted.root.querySelector<HTMLInputElement>("#vault-saved")!.checked = true;
  entered.value = "synthetic-code-1";
  restarted.root.querySelector<HTMLFormElement>("#vault-confirm-form")!.dispatchEvent(
    new restarted.dom.window.Event("submit", { bubbles: true, cancelable: true }),
  );
  await delay(0);
  assert.equal(published, false);
  assert.match(restarted.root.querySelector("#vault-error")?.textContent ?? "", /confirmation failed/i);
  entered.value = "synthetic-code-2";
  restarted.root.querySelector<HTMLFormElement>("#vault-confirm-form")!.dispatchEvent(
    new restarted.dom.window.Event("submit", { bubbles: true, cancelable: true }),
  );
  await delay(0);
  assert.equal(published, true);
  assert.equal(opened, 0);
  assert.ok(restarted.root.querySelector("#security-form"));
  restarted.root.querySelector<HTMLInputElement>("#security-secret")!.value = secret;
  restarted.root.querySelector<HTMLFormElement>("#security-form")!.dispatchEvent(
    new restarted.dom.window.Event("submit", { bubbles: true, cancelable: true }),
  );
  await delay(0);
  assert.equal(opened, 1);
  assert.ok(calls.includes("confirm_vault_setup"));
});

test("configured vault with missing or corrupt manifest never enters provisioning", async () => {
  const { dom, root } = domRoot();
  const calls: string[] = [];
  const invoke: Invoke = async <T>(command: string) => {
    calls.push(command);
    if (command === "get_security_status") return { ...lockedConfigured, vault_mode: "encrypted" } as T;
    if (command === "unlock_security") throw new Error("MP_SECURITY_IO:corrupt vault manifest");
    throw new Error("unexpected " + command);
  };
  await mountSecurityGate({ root, invoke, onUnlocked: () => assert.fail("must remain locked") });
  root.querySelector<HTMLInputElement>("#security-secret")!.value = "correct synthetic secret";
  root.querySelector<HTMLFormElement>("#security-form")!.dispatchEvent(
    new dom.window.Event("submit", { bubbles: true, cancelable: true }),
  );
  await delay(0);
  assert.equal(root.querySelector("#vault-code"), null);
  assert.ok(!calls.includes("begin_vault_setup"));
  assert.match(root.querySelector("#security-error")?.textContent ?? "", /stayed locked/i);
});

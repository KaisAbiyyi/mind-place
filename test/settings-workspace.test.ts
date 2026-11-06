import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import { mountSettingsWorkspace, fetchHealthSummary } from "../src/settings-workspace.js";
import type { Invoke } from "../src/reader.js";

function createFixture(invoke: Invoke, options = {}) {
  const dom = new JSDOM("<!doctype html><html><body><main id='view'></main></body></html>", { url: "https://local.test/" });
  const view = dom.window.document.querySelector<HTMLElement>("#view")!;
  const handle = mountSettingsWorkspace(view, invoke, options);
  return { dom, view, handle };
}

test("MP-UI-08: General preferences toggle theme, density, and technical mode", async () => {
  const invoke: Invoke = async <T>(command: string) => {
    if (command === "get_security_status") return { configured: true, locked: false, epoch: 2 } as T;
    if (command === "get_archive_index_stats") return { sources: 10, revisions: 15, chunks: 80, generation: 4 } as T;
    if (command === "get_local_mcp_status") return { running: true } as T;
    if (command === "get_hosted_tunnel_status") return { running: true, connection_verified: true } as T;
    throw new Error("Unhandled: " + command);
  };

  const { dom, view, handle } = createFixture(invoke);
  await delay(10);

  const doc = dom.window.document;

  // Toggle theme
  const themeToggle = view.querySelector<HTMLButtonElement>("[data-action='toggle-theme-setting']")!;
  assert.ok(themeToggle);
  themeToggle.click();
  assert.equal(doc.documentElement.dataset.theme, "dark");

  // Toggle density
  const densityToggle = view.querySelector<HTMLButtonElement>("[data-action='toggle-density-setting']")!;
  assert.ok(densityToggle);
  densityToggle.click();
  assert.equal(doc.documentElement.dataset.density, "compact");

  // Toggle technical mode
  const techToggle = view.querySelector<HTMLButtonElement>("[data-action='toggle-tech-setting']")!;
  assert.ok(techToggle);
  techToggle.click();
  assert.equal(doc.documentElement.dataset.technicalMode, "true");

  handle.destroy();
});

test("MP-UI-08: Storage & Vault displays factual state, recovery ceremony, backup and boundaries", async () => {
  const commands: string[] = [];
  const invoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
    commands.push(command);
    if (command === "get_security_status") return { configured: true, locked: false, epoch: 3, vault_mode: "encrypted" } as T;
    if (command === "get_archive_index_stats") return { sources: 5, revisions: 8, chunks: 40, generation: 2 } as T;
    if (command === "get_local_mcp_status") return { running: false } as T;
    if (command === "get_hosted_tunnel_status") return { running: false, connection_verified: false } as T;
    if (command === "begin_vault_setup") {
      assert.equal(args?.secret, "mypassword123");
      return "CODE-RECOVER-XYZ-789" as T;
    }
    if (command === "confirm_vault_setup") {
      assert.equal(args?.code, "CODE-RECOVER-XYZ-789");
      return {} as T;
    }
    if (command === "create_vault_online_backup") {
      assert.equal(args?.destination, "D:\\backups\\mp");
      assert.equal(args?.generation, 2);
      return { version: 1, vault_id_b64: "synthetic", generation: 2, count: 40 } as T;
    }
    throw new Error("Unhandled: " + command);
  };

  const { view, handle } = createFixture(invoke, { initialSection: "storage-vault" });
  await delay(10);

  // Check factual vault status
  assert.match(view.textContent!, /Security Epoch:3/);
  assert.match(view.textContent!, /Configured/);
  assert.match(view.textContent!, /Unlocked/);

  // Check boundary explanation
  assert.match(view.textContent!, /MP-9C backend acceptance/);

  assert.equal(view.querySelector("input[type=password]"), null);
  assert.equal(commands.includes("begin_vault_setup"), false);

  // Test online backup
  const destInput = view.querySelector<HTMLInputElement>("label:has(input[placeholder*='backups']) input") ??
    view.querySelector<HTMLInputElement>("input[placeholder*='backups']")!;
  destInput.value = "D:\\backups\\mp";
  const genInput = view.querySelector<HTMLInputElement>("input[type='number'][min='1']")!;
  genInput.value = "2";

  const backupBtn = [...view.querySelectorAll<HTMLButtonElement>("button")].find(
    (b) => b.textContent?.includes("Create Online Backup"),
  )!;
  backupBtn.click();

  // Click proceed in confirm dialog
  const backupProceedBtn = view.querySelector<HTMLButtonElement>("#settings-confirm-dialog .ui-button-primary")!;
  backupProceedBtn.click();
  await delay(10);

  assert.ok(commands.includes("create_vault_online_backup"));
  assert.match(view.textContent!, /Generation 2; 40 objects/);

  handle.destroy();
});

test("MP-UI-08: Privacy & Security section triggers session lock", async () => {
  let locked = false;
  const invoke: Invoke = async <T>(command: string) => {
    if (command === "get_security_status") return { configured: true, locked: false, epoch: 5 } as T;
    if (command === "get_archive_index_stats") return { sources: 0, revisions: 0, chunks: 0, generation: 1 } as T;
    if (command === "get_local_mcp_status" || command === "get_hosted_tunnel_status") return { running: false } as T;
    throw new Error("Unhandled: " + command);
  };

  const { view, handle } = createFixture(invoke, {
    initialSection: "privacy",
    onLock: () => {
      locked = true;
    },
  });
  await delay(10);

  assert.match(view.textContent!, /Current Unlock Epoch: 5/);
  const lockBtn = view.querySelector<HTMLButtonElement>("[data-action='lock-mind-place']")!;
  lockBtn.click();
  assert.equal(locked, true);

  handle.destroy();
});

test("MP-UI-08: Operational diagnostics rechecks index stats, rebuilds index, and traces query without secret leak", async () => {
  const commands: string[] = [];
  const invoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
    commands.push(command);
    if (command === "get_security_status") return { configured: true, locked: false, epoch: 4 } as T;
    if (command === "get_archive_index_stats") return { sources: 12, revisions: 18, chunks: 95, generation: 3 } as T;
    if (command === "get_local_mcp_status") return { running: true, token: "LEAKED_SECRET_TOKEN" } as T;
    if (command === "get_hosted_tunnel_status") return { running: true, connection_verified: true, secret: "TUNNEL_SECRET" } as T;
    if (command === "rebuild_archive_index") {
      return { sources: 12, revisions: 18, chunks: 95, generation: 4 } as T;
    }
    if (command === "inspect_retrieval") {
      return {
        trace: {
          operation: "search",
          elapsed_us: 380,
          index_generation: 3,
          candidate_count: 8,
          result_count: 2,
          source_count: 2,
          estimated_tokens: 150,
          truncated: false,
        },
      } as T;
    }
    throw new Error("Unhandled: " + command);
  };

  const { dom, view, handle } = createFixture(invoke, { initialSection: "diagnostics" });
  await delay(10);

  // Check metrics rendered
  assert.match(view.textContent!, /Generation 3/);
  assert.match(view.textContent!, /Process running/);
  assert.match(view.textContent!, /connector reports verified/);

  // Ensure secrets were NEVER leaked
  assert.doesNotMatch(view.textContent!, /LEAKED_SECRET_TOKEN/);
  assert.doesNotMatch(view.textContent!, /TUNNEL_SECRET/);

  // Test rebuild archive index with confirmation
  const rebuildBtn = [...view.querySelectorAll<HTMLButtonElement>("button")].find(
    (b) => b.textContent?.includes("Rebuild Archive Index"),
  )!;
  rebuildBtn.click();

  const proceedBtn = view.querySelector<HTMLButtonElement>("#settings-confirm-dialog .ui-button-primary")!;
  proceedBtn.click();
  await delay(10);

  assert.ok(commands.includes("rebuild_archive_index"));
  assert.match(view.textContent!, /Generation 4/);

  // Test query tracer
  const tracerInput = view.querySelector<HTMLInputElement>(".settings-tracer-form input[type='search']")!;
  tracerInput.value = "test trace query";
  view.querySelector(".settings-tracer-form")!.dispatchEvent(new dom.window.Event("submit", { bubbles: true, cancelable: true }));
  await delay(10);

  assert.ok(commands.includes("inspect_retrieval"));
  assert.match(view.textContent!, /380 µs/);
  assert.match(view.textContent!, /Candidates Evaluated/);

  handle.destroy();
  assert.equal(view.textContent, "");
});

test("MP-UI-08: fetchHealthSummary aggregates factual metrics cleanly", async () => {
  const invoke: Invoke = async <T>(command: string) => {
    if (command === "get_security_status") return { configured: true, locked: false, epoch: 9 } as T;
    if (command === "get_archive_index_stats") return { sources: 100, revisions: 200, chunks: 500, generation: 12 } as T;
    if (command === "get_local_mcp_status") return { running: true } as T;
    if (command === "get_hosted_tunnel_status") return { running: true, connection_verified: true } as T;
    throw new Error("Unhandled: " + command);
  };

  const summary = await fetchHealthSummary(invoke);
  assert.ok(summary);
  assert.equal(summary?.securityEpoch, 9);
  assert.equal(summary?.indexGeneration, 12);
  assert.equal(summary?.indexSources, 100);
  assert.equal(summary?.localMcpRunning, true);
  assert.equal(summary?.hostedTunnelVerified, true);
});

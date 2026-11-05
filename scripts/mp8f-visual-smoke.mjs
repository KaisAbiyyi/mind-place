/**
 * MP-8F cloud-only synthetic Chromium visual/keyboard smoke.
 * Does NOT qualify installed Windows WebView2 or read any private archive.
 * Artifacts contain only fake empty-state content and UI layout.
 */
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "@playwright/test";

const base = "http://127.0.0.1:1420";
const output = "artifacts/mp8f-visual";
const server = spawn(process.execPath, ["node_modules/vite/bin/vite.js", "--host", "127.0.0.1", "--port", "1420", "--strictPort"], {
  stdio: ["ignore", "pipe", "pipe"],
});
const messages = [];
server.stdout.on("data", chunk => messages.push(String(chunk)));
server.stderr.on("data", chunk => messages.push(String(chunk)));
let browser;
let error;
try {
  let ready = false;
  for (let i = 0; i < 100; i++) {
    if (server.exitCode !== null) throw new Error("Vite exited early: " + messages.join("").slice(-1000));
    try { const result = await fetch(base); if (result.ok) { ready = true; break; } } catch { /* warming up */ }
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  assert.ok(ready, "Vite dev server did not start");
  await mkdir(output, { recursive: true });
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1180, height: 820 } });
  const faults = [];
  page.on("pageerror", e => faults.push(e.message));
  await page.goto(base, { waitUntil: "networkidle" });
  // Replace only the failed/locked demo gate with an isolated, synthetic reader.
  // Never simulate a successful native security unlock or filesystem authority.
  await page.evaluate(async () => {
    const { mountReader } = await import("/src/reader.ts");
    const root = document.querySelector("#app");
    const invoke = async command => {
      if (["list_conversations", "list_memory_inspection"].includes(command)) return [];
      if (command === "get_security_status") return { configured: true, locked: false, epoch: 123 };
      if (command === "get_archive_index_stats") return { sources: 0, revisions: 0, chunks: 0, generation: 0 };
      if (command === "get_local_mcp_status" || command === "get_hosted_tunnel_status") return { running: false, connection_verified: false };
      throw new Error("Synthetic fixture does not implement " + command);
    };
    window.__mp8f_reader = mountReader({ root, invoke, onLock: () => {} });
    await window.__mp8f_reader.ready;
  });

  const receipt = [];
  const sizes = [[1180, 820], [800, 520], [390, 700], [360, 520]];
  for (const [width, height] of sizes) {
    await page.setViewportSize({ width, height });
    for (const name of ["library", "memories", "graph", "health"]) {
      await page.locator('[data-workspace="' + name + '"]').click();
      await page.waitForTimeout(60);
      const metrics = await page.evaluate(expected => {
        const view = document.querySelector("#reader-view");
        const heading = view.querySelector("h1");
        const selected = document.querySelectorAll('#knowledge-os-nav [aria-current="page"]');
        const screen = document.documentElement;
        const rect = heading?.getBoundingClientRect();
        return {
          selected: selected.length === 1 && selected[0].getAttribute("data-workspace") === expected,
          heading: Boolean(rect && rect.width > 40 && rect.left >= 0 && rect.right <= window.innerWidth + 1),
          horizontalOverflow: screen.scrollWidth > window.innerWidth + 1,
          scrollHeight: view.scrollHeight, clientHeight: view.clientHeight,
        };
      }, name);
      assert.equal(metrics.selected, true, width + "x" + height + " / " + name + " navigation");
      assert.equal(metrics.heading, true, width + "x" + height + " / " + name + " heading");
      assert.equal(metrics.horizontalOverflow, false, width + "x" + height + " / " + name + " viewport overflow");
      await page.screenshot({ path: output + "/" + width + "x" + height + "-" + name + ".png" });
      receipt.push({ viewport: width + "x" + height, workspace: name, ...metrics });
    }
  }
  // MP-8L1: Windows DPI of 100/125/150/175% reduces available CSS pixels.
  // Do not let fixed sidebar sections shrink the archive viewport to zero.
  // Chromium can catch layout regressions; the owner still qualifies native WebView2.
  const desktopScalingSizes = [
    [1700, 900, "100%"], [1360, 720, "125%"],
    [1133, 600, "150%"], [971, 514, "175%"],
  ];
  for (const [width, height, label] of desktopScalingSizes) {
    await page.setViewportSize({ width, height });
    const metrics = await page.evaluate(() => {
      const sidebar = document.querySelector(".library");
      const list = document.querySelector("#conversation-list");
      const footer = document.querySelector(".library-footer");
      const listHeight = list.getBoundingClientRect().height;
      const scrollable = sidebar.scrollHeight > sidebar.clientHeight;
      list.scrollIntoView({ block: "center" });
      const listBounds = list.getBoundingClientRect();
      const sidebarBounds = sidebar.getBoundingClientRect();
      const visibleListHeight = Math.max(0, Math.min(listBounds.bottom, sidebarBounds.bottom) -
        Math.max(listBounds.top, sidebarBounds.top));
      sidebar.scrollTop = sidebar.scrollHeight;
      const footerBounds = footer.getBoundingClientRect();
      const footerReachable = footerBounds.top < sidebarBounds.bottom + 1 &&
        footerBounds.bottom > sidebarBounds.top;
      return { listHeight, visibleListHeight, scrollable, footerReachable };
    });
    const minHeight = label === "150%" || label === "175%" ? 164 : 76;
    assert.ok(metrics.listHeight >= minHeight - 1, label + " conversation list collapsed: " + JSON.stringify(metrics));
    assert.ok(metrics.visibleListHeight >= 76, label + " conversation list inaccessible: " + JSON.stringify(metrics));
    assert.equal(metrics.footerReachable, true, label + " footer inaccessible");
    if (label === "150%" || label === "175%") {
      assert.equal(metrics.scrollable, true, label + " should allow scrolling the full sidebar");
    }
    receipt.push({ viewport: width + "x" + height, desktopScaling: label, ...metrics });
  }
  await page.setViewportSize({ width: 1180, height: 820 });

  await page.locator('[data-workspace="connections"]').click();
  assert.equal(await page.locator("#setup-dialog").getAttribute("open"), "");
  await page.keyboard.press("Escape");
  assert.equal(await page.locator("#setup-dialog").getAttribute("open"), null);
  assert.equal(await page.locator('#knowledge-os-nav [aria-current="page"]').getAttribute("data-workspace"), "health");
  assert.deepEqual(faults, [], "unexpected client JS errors");
  await writeFile(output + "/receipt.json", JSON.stringify({
    status: "PASS", platform: "cloud Chromium (NOT installed Windows WebView2)",
    source: "synthetic empty fixture", sizes: receipt,
  }, null, 2) + "\n");
  console.log("MP-8F cloud synthetic viewport and keyboard smoke PASS, " + receipt.length + " screenshots.");
} catch (caught) {
  error = caught;
  await mkdir(output, { recursive: true });
  await writeFile(output + "/failure.txt", String(caught.stack ?? caught) + "\n").catch(() => {});
} finally {
  await browser?.close();
  server.kill("SIGTERM");
}
if (error) { console.error(error); process.exitCode = 1; }

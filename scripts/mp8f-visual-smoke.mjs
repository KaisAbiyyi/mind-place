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
    const navigate = {
      library: async () => {
        await page.locator('#knowledge-os-nav [data-route="overview"]').click();
        const libraryAction = page.locator('#knowledge-os-nav [data-nav-secondary][data-workspace="library"]');
        try {
          await libraryAction.click({ timeout: 10000 });
        } catch (failure) {
          const hitTest = await page.evaluate(() => {
            const element = document.querySelector('#knowledge-os-nav [data-nav-secondary][data-workspace="library"]');
            if (!element) return { missing: true };
            const rect = element.getBoundingClientRect();
            const x = rect.left + rect.width / 2, y = rect.top + rect.height / 2;
            const identity = (node) => ({ tag: node.tagName, id: node.id, className: typeof node.className === "string" ? node.className : "" });
            const ancestors = [];
            for (let node = element; node && ancestors.length < 6; node = node.parentElement) {
              const style = getComputedStyle(node);
              ancestors.push({ ...identity(node), width: Math.round(node.getBoundingClientRect().width),
                scrollWidth: node.scrollWidth, clientWidth: node.clientWidth, scrollLeft: node.scrollLeft,
                overflowX: style.overflowX, flexShrink: style.flexShrink });
            }
            return { viewport: window.innerWidth, actionRect: { left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom },
              hitStack: document.elementsFromPoint(x, y).slice(0, 6).map(identity), ancestors };
          });
          console.error("MP-8F Library navigation pointer diagnostic: " + JSON.stringify(hitTest));
          throw failure;
        }
      },
      memories: async () => page.locator('#knowledge-os-nav [data-route="curated-memory"]').click(),
      graph: async () => page.locator('#knowledge-os-nav [data-route="hive-mind-graph"]').click(),
      health: async () => {
        await page.locator('#knowledge-os-nav [data-route="settings"]').click();
        await page.locator('#knowledge-os-nav [data-nav-secondary][data-workspace="health"]').click();
      },
    };
    for (const name of ["library", "memories", "graph", "health"]) {
      await navigate[name]();
      await page.waitForTimeout(60);
      const metrics = await page.evaluate(expected => {
        const view = document.querySelector("#reader-view");
        const heading = view.querySelector("h1");
        const selected = document.querySelectorAll('#knowledge-os-nav [aria-current="page"]');
        const screen = document.documentElement;
        const rect = heading?.getBoundingClientRect();
        return {
          selected: selected.length === 1 && selected[0].getAttribute("data-route") === ({ library: "overview", memories: "curated-memory", graph: "hive-mind-graph", health: "settings" })[expected],
          heading: Boolean(rect && rect.width > 40 && rect.left >= 0 && rect.right <= window.innerWidth + 1),
          horizontalOverflow: screen.scrollWidth > window.innerWidth + 1,
          viewportScrollWidth: screen.scrollWidth, viewportWidth: window.innerWidth,
          horizontalScrollNodes: [...document.querySelectorAll("*")].filter((el) => el.scrollWidth > el.clientWidth + 1)
            .slice(0, 24).map((el) => {
              const bounds = el.getBoundingClientRect(), styles = getComputedStyle(el);
              return { tag: el.tagName, id: el.id, className: typeof el.className === "string" ? el.className : "",
                left: Math.round(bounds.left), right: Math.round(bounds.right),
                scrollWidth: el.scrollWidth, clientWidth: el.clientWidth,
                overflowX: styles.overflowX, minWidth: styles.minWidth };
            }),
          overflowNodes: [...document.querySelectorAll("*")].filter((element) => {
            const bounds = element.getBoundingClientRect();
            return bounds.right > window.innerWidth + 1 || bounds.left < -1;
          }).slice(0, 12).map((element) => ({
            tag: element.tagName, id: element.id, className: typeof element.className === "string" ? element.className : "",
            left: Math.round(element.getBoundingClientRect().left), right: Math.round(element.getBoundingClientRect().right),
            width: Math.round(element.getBoundingClientRect().width), scrollWidth: element.scrollWidth, clientWidth: element.clientWidth,
          })),
          scrollHeight: view.scrollHeight, clientHeight: view.clientHeight,
        };
      }, name);
      await page.screenshot({ path: output + "/" + width + "x" + height + "-" + name + ".png" });
      assert.equal(metrics.selected, true, width + "x" + height + " / " + name + " navigation: " + JSON.stringify(metrics));
      assert.equal(metrics.heading, true, width + "x" + height + " / " + name + " heading: " + JSON.stringify(metrics));
      assert.equal(metrics.horizontalOverflow, false, width + "x" + height + " / " + name + " viewport overflow: " + JSON.stringify(metrics));
      receipt.push({ viewport: width + "x" + height, workspace: name, ...metrics });
    }
  }
  await page.locator('#knowledge-os-nav [data-route="ai-providers-mcp"]').click();
  assert.equal(await page.locator(".providers-workspace h1").textContent(), "AI Providers & MCP Authorization");
  assert.equal(await page.locator("#setup-dialog").getAttribute("open"), null);
  await page.locator('.providers-workspace [data-action="open-setup"]').click();
  assert.equal(await page.locator("#setup-dialog").getAttribute("open"), "");
  await page.keyboard.press("Escape");
  assert.equal(await page.locator("#setup-dialog").getAttribute("open"), null);
  assert.equal(await page.locator('#knowledge-os-nav [aria-current="page"]').getAttribute("data-route"), "ai-providers-mcp");
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

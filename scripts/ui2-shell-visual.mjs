import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import assert from "node:assert/strict";
import { chromium } from "@playwright/test";
import { createServer } from "vite";

const rootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const outDir = path.join(rootDir, "docs", "ui", "evidence", "mp-ui2", "shell");

const HTML = `<!doctype html>
<html lang="en" data-theme="light" data-density="comfortable">
<head><meta charset="utf-8" /><meta name="viewport" content="width=device-width,initial-scale=1" />
<style>html,body{margin:0;min-height:100%}</style></head>
<body><div id="app"></div>
<script type="module">
  import { createI18nController } from "/src/ui2/i18n/index.ts";
  import { mountUi2Shell } from "/src/ui2/shell/entry.ts";
  const params = new URLSearchParams(location.search);
  document.documentElement.dataset.theme = params.get("theme") || "light";
  const locale = params.get("locale") || "en";
  const i18n = createI18nController({ initialLocale: locale });
  const root = document.getElementById("app");
  window.__shell = mountUi2Shell(root, { i18n, onLock() { window.__locked = true; },
    onSwitchClassic() { window.__classicRequested = true; } });
</script></body></html>`;

const entryPlugin = {
  name: "ui2-shell-virtual-entry",
  configureServer(server) {
    server.middlewares.use(async (req, res, next) => {
      if (req.url?.split("?")[0] !== "/__ui2-shell.html") return next();
      const html = await server.transformIndexHtml(req.url, HTML);
      res.statusCode = 200;
      res.setHeader("Content-Type", "text/html; charset=utf-8");
      res.end(html);
    });
  },
};

async function audit(page) {
  return page.evaluate(() => {
    const shell = document.querySelector(".ui2-shell");
    const navButtons = [...document.querySelectorAll(".ui2-shell-nav-button")];
    const missingAccessibleNames = [...document.querySelectorAll(".ui2-root button")]
      .filter(el => {
        const rect = el.getBoundingClientRect();
        if (!rect.width && !rect.height) return false;
        return !(el.getAttribute("aria-label") || el.textContent?.trim());
      }).map(el => el.className || el.tagName);
    const navNames = navButtons.map(el => el.getAttribute("aria-label"));
    const rect = shell?.getBoundingClientRect();
    const visibleText = shell?.innerText || "";
    return {
      destinations: navButtons.length,
      navNames,
      overflowPx: Math.max(0, document.documentElement.scrollWidth - window.innerWidth),
      minRootWidth: rect?.width ?? 0,
      missingAccessibleNames,
      technicalStringVisible: /\\b(?:sha-?256|revision|epoch|grpc|sqlite|token budget)\\b/i.test(visibleText),
      unavailableState: Boolean(document.querySelector(".ui2-shell-placeholder")),
      nonemptyPreview: Boolean(document.querySelector(".ui2-shell-placeholder .ui2-empty-body")?.textContent?.trim()),
      documentLanguage: document.documentElement.lang,
      styleLoaded: getComputedStyle(shell).display === "grid",
    };
  });
}

async function main() {
  fs.mkdirSync(outDir, { recursive: true });
  const server = await createServer({ root: rootDir, server: { host: "127.0.0.1", port: 0 }, plugins: [entryPlugin], logLevel: "silent" });
  await server.listen();
  const address = server.httpServer.address();
  const url = `http://127.0.0.1:${address.port}/__ui2-shell.html`;
  const browser = await chromium.launch({ headless: true });
  const matrix = [];
  const interactions = [];

  try {
    const viewports = [
      { name: "wide", width: 1280, height: 840 },
      { name: "laptop", width: 1024, height: 768 },
      { name: "narrow", width: 760, height: 768 },
    ];
    for (const theme of ["light", "dark"]) {
      for (const locale of ["en", "id", "pseudo"]) {
        for (const viewport of viewports) {
          const ctx = await browser.newContext({ viewport });
          const page = await ctx.newPage();
          const errors = [];
          page.on("pageerror", error => errors.push(error.message));
          await page.goto(`${url}?theme=${theme}&locale=${locale}`, { waitUntil: "networkidle" });
          await page.waitForSelector(".ui2-shell-nav-button");
          await page.waitForFunction(() => getComputedStyle(document.querySelector(".ui2-shell")).display === "grid");
          const result = await audit(page);
          assert.deepEqual(errors, [], `browser errors ${theme}/${locale}/${viewport.name}`);
          assert.equal(result.destinations, 7);
          assert.equal(result.overflowPx, 0, `horizontal overflow ${theme}/${locale}/${viewport.name}`);
          assert.deepEqual(result.missingAccessibleNames, []);
          assert.equal(result.technicalStringVisible, false);
          assert.equal(result.unavailableState && result.nonemptyPreview && result.styleLoaded, true);
          assert.equal(result.documentLanguage, locale === "id" ? "id" : "en");
          const filename = `shell-${theme}-${locale}-${viewport.name}.png`;
          await page.screenshot({ path: path.join(outDir, filename), fullPage: true });
          matrix.push({ filename, theme, locale, viewport, audit: result });
          await ctx.close();
        }
      }
    }

    const ctx = await browser.newContext({ viewport: { width: 1280, height: 840 } });
    const page = await ctx.newPage();
    await page.goto(`${url}?theme=light&locale=en`, { waitUntil: "networkidle" });

    await page.locator('[data-ui2-route="/memory"]').click();
    assert.equal(await page.locator(".ui2-breadcrumb-current").innerText(), "Memory");
    await page.getByRole("button", { name: "Home", exact: true }).last().click();
    assert.equal(await page.locator(".ui2-breadcrumb-current").innerText(), "Home");

    const searchButton = page.locator('[data-ui2-control="search"]');
    await searchButton.focus();
    await page.keyboard.press("Control+k");
    assert.equal(await page.getByRole("dialog").isVisible(), true);
    const filenameSearch = "shell-state-search-light-en-wide.png";
    await page.screenshot({ path: path.join(outDir, filenameSearch) });
    interactions.push(filenameSearch);
    await page.keyboard.press("Escape");
    assert.equal(await searchButton.evaluate(el => document.activeElement === el), true);

    await page.locator('[data-ui2-seg-value="id"]').click();
    assert.equal(await page.locator('[data-ui2-route="/home"]').getAttribute("aria-label"), "Beranda");
    await page.locator('[data-ui2-seg-value="en"]').click();
    await page.locator('[data-ui2-control="sidebar"]').click();
    assert.equal(await page.locator(".ui2-shell").getAttribute("data-collapsed"), "true");
    const filenameCollapsed = "shell-state-collapsed-light-en-wide.png";
    await page.screenshot({ path: path.join(outDir, filenameCollapsed) });
    interactions.push(filenameCollapsed);
    await ctx.close();

    const mobileCtx = await browser.newContext({ viewport: { width: 760, height: 768 } });
    const mobile = await mobileCtx.newPage();
    await mobile.goto(`${url}?theme=dark&locale=id`, { waitUntil: "networkidle" });
    const mobileToggle = mobile.locator('[data-ui2-control="sidebar"]');
    await mobileToggle.click();
    assert.equal(await mobile.locator(".ui2-shell").getAttribute("data-mobile-open"), "true");
    const filenameMobile = "shell-state-mobile-nav-dark-id-narrow.png";
    await mobile.screenshot({ path: path.join(outDir, filenameMobile) });
    interactions.push(filenameMobile);
    await mobile.keyboard.press("Escape");
    assert.equal(await mobile.locator(".ui2-shell").getAttribute("data-mobile-open"), "false");
    assert.equal(await mobileToggle.evaluate(el => document.activeElement === el), true);
    await mobileCtx.close();

    const manifest = {
      guide: "docs/ui/evidence/mp-ui2/wireframe-v1.html",
      guideSha256: "2b4bbce50a079516e7db2326345cfd0bd227ff7b6a5dd347616fc9c790bc3a8c",
      generatedAt: new Date().toISOString(),
      totalCaptures: matrix.length + interactions.length,
      matrix, interactions,
      browserNavigation: "PASS",
      ctrlKAndEscapeFocusReturn: "PASS",
      ENIDSwitch: "PASS",
      mobileOverlayEscapeFocusReturn: "PASS",
      contrastAudit: "NOT_RUN (manual design review required)",
      nativeWindowsWebView2: "NOT_RUN (owner gate)",
    };
    fs.writeFileSync(path.join(outDir, "shell-manifest.json"), JSON.stringify(manifest, null, 2));
    console.log(`UI2 shell visuals PASS: ${manifest.totalCaptures} captures; 0 overflow; 7 destinations; accessible names; keyboard and focus checks`);
  } finally {
    await browser.close();
    await server.close();
  }
}
main().catch(err => { console.error(err); process.exitCode = 1; });

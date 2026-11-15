import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";
import { createServer } from "vite";

const rootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const outDir = path.join(rootDir, "docs", "ui", "evidence", "mp-ui2", "gallery");

const GALLERY_HTML = `<!DOCTYPE html>
<html lang="en" data-theme="light" data-density="comfortable">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>Mind Place — Design System v2 Gallery</title>
  <link rel="stylesheet" href="/src/ui2/ui-kit/tokens.css" />
  <link rel="stylesheet" href="/src/ui2/ui-kit/primitives.css" />
</head>
<body>
  <div id="gallery-root"></div>
  <script type="module">
    import { createI18nController } from "/src/ui2/i18n/index.ts";
    import { mountUi2Gallery } from "/src/ui2/ui-kit/gallery.ts";

    const params = new URLSearchParams(window.location.search);
    const locale = params.get("locale") || "en";
    const theme = params.get("theme") || "light";
    const density = params.get("density") || "comfortable";

    const i18n = createI18nController({ initialLocale: locale });
    const root = document.getElementById("gallery-root");
    const handle = mountUi2Gallery(root, {
      i18n,
      initialTheme: theme,
      initialDensity: density,
    });
    window.__ui2Gallery = handle;
  </script>
</body>
</html>`;

const galleryEntryPlugin = {
  name: "ui2-gallery-virtual-entry",
  configureServer(server) {
    server.middlewares.use(async (req, res, next) => {
      const url = (req.url || "").split("?")[0];
      if (url === "/__ui2-gallery.html") {
        const html = await server.transformIndexHtml(req.url, GALLERY_HTML);
        res.statusCode = 200;
        res.setHeader("Content-Type", "text/html; charset=utf-8");
        res.end(html);
        return;
      }
      next();
    });
  },
};

async function runAuditAndChecks(page) {
  return page.evaluate(() => {
    const parseColor = (str) => {
      const m = str.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([0-9.]+))?\)/);
      if (!m) return { r: 0, g: 0, b: 0, a: 1 };
      return {
        r: Number(m[1]),
        g: Number(m[2]),
        b: Number(m[3]),
        a: m[4] === undefined ? 1 : Number(m[4]),
      };
    };

    const blendOver = (fg, bg) => {
      const a = fg.a;
      return {
        r: Math.round(fg.r * a + bg.r * (1 - a)),
        g: Math.round(fg.g * a + bg.g * (1 - a)),
        b: Math.round(fg.b * a + bg.b * (1 - a)),
        a: 1,
      };
    };

    const relLuminance = ({ r, g, b }) => {
      const toLinear = (c) => {
        const s = c / 255;
        return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
      };
      return 0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);
    };

    const contrastRatio = (c1, c2) => {
      const l1 = relLuminance(c1);
      const l2 = relLuminance(c2);
      const hi = Math.max(l1, l2);
      const lo = Math.min(l1, l2);
      return (hi + 0.05) / (lo + 0.05);
    };

    const effectiveBg = (el) => {
      const chain = [];
      let curr = el;
      while (curr) {
        chain.unshift(parseColor(getComputedStyle(curr).backgroundColor));
        curr = curr.parentElement;
      }
      const theme = document.documentElement.getAttribute("data-theme");
      let acc = theme === "dark" ? { r: 9, g: 9, b: 11, a: 1 } : { r: 250, g: 250, b: 250, a: 1 };
      for (const layer of chain) {
        if (layer.a > 0) acc = blendOver(layer, acc);
      }
      return acc;
    };

    const overflowPx = Math.max(0, document.documentElement.scrollWidth - window.innerWidth);

    // Accessible name check across visible controls
    const missingA11yNames = [];
    for (const el of document.querySelectorAll("button, input, [role='switch'], [role='radio']")) {
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 && rect.height === 0) continue;
      const cs = getComputedStyle(el);
      if (cs.visibility === "hidden" || cs.display === "none") continue;
      const aria = el.getAttribute("aria-label") || el.getAttribute("aria-labelledby") || "";
      const text = (el.innerText || "").trim();
      let labelText = "";
      if (el instanceof HTMLInputElement && el.id) {
        labelText = (document.querySelector(`label[for="${el.id}"]`)?.textContent || "").trim();
      }
      if (!aria && !text && !labelText) {
        missingA11yNames.push(el.tagName + "." + el.className);
      }
    }

    // Technical details default visibility check
    const techDetails = [...document.querySelectorAll('[data-ui2-technical-details="true"]')];
    const openTechDetailsCount = techDetails.filter((d) => d.hasAttribute("open")).length;
    const anyTechCodeVisibleOutsideOpen = [...document.querySelectorAll('[data-ui2-technical-code="true"]')]
      .filter((code) => {
        const parentDetails = code.closest('[data-ui2-technical-details="true"]');
        if (!parentDetails) return true;
        return parentDetails.hasAttribute("open");
      })
      .map((c) => c.textContent || "");

    // Contrast ratio audit on visible text nodes inside .ui2-root
    let minContrast = 21;
    const contrastFailures = [];
    const walker = document.createTreeWalker(document.querySelector(".ui2-root"), NodeFilter.SHOW_TEXT);
    let node;
    while ((node = walker.nextNode())) {
      if (!node.textContent || !node.textContent.trim()) continue;
      const el = node.parentElement;
      if (!el) continue;
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) continue;
      const cs = getComputedStyle(el);
      if (cs.visibility === "hidden" || cs.display === "none" || cs.opacity === "0") continue;
      const detailsParent = el.closest("details");
      if (detailsParent && !detailsParent.open && el.tagName.toLowerCase() !== "summary" && !el.closest("summary")) {
        continue;
      }
      const fg = parseColor(cs.color);
      const bg = effectiveBg(el);
      const ratio = contrastRatio(blendOver(fg, bg), bg);
      const sizePx = parseFloat(cs.fontSize);
      const weight = Number(cs.fontWeight);
      const isLarge = sizePx >= 24 || (sizePx >= 18.66 && weight >= 600);
      const required = isLarge ? 3.0 : 4.5;
      if (ratio < minContrast) minContrast = ratio;
      if (ratio < required) {
        contrastFailures.push({
          text: node.textContent.trim().slice(0, 40),
          className: el.className,
          ratio: Number(ratio.toFixed(2)),
          required,
        });
      }
    }

    return {
      overflowPx,
      missingA11yNames,
      openTechDetailsCount,
      anyTechCodeVisibleOutsideOpen,
      minContrast: Number(minContrast.toFixed(2)),
      contrastFailures,
    };
  });
}

async function main() {
  fs.mkdirSync(outDir, { recursive: true });

  const viteServer = await createServer({
    root: rootDir,
    server: { host: "127.0.0.1", port: 0, strictPort: false },
    plugins: [galleryEntryPlugin],
    logLevel: "silent",
  });
  await viteServer.listen();
  const address = viteServer.httpServer.address();
  const baseUrl = `http://127.0.0.1:${address.port}/__ui2-gallery.html`;

  const browser = await chromium.launch({ headless: true });
  const results = [];

  try {
    const viewports = [
      { name: "wide", width: 1280, height: 840 },
      { name: "laptop", width: 1024, height: 768 },
      { name: "narrow", width: 760, height: 768 },
    ];

    for (const theme of ["light", "dark"]) {
      for (const locale of ["en", "id"]) {
        for (const vp of viewports) {
          const context = await browser.newContext({
            viewport: { width: vp.width, height: vp.height },
          });
          const page = await context.newPage();
          await page.goto(`${baseUrl}?theme=${theme}&locale=${locale}`, {
            waitUntil: "networkidle",
          });
          await page.waitForSelector(".ui2-root");

          const audit = await runAuditAndChecks(page);
          if (audit.overflowPx > 0) {
            throw new Error(`Horizontal overflow (${audit.overflowPx}px) at ${theme}/${locale}/${vp.name}`);
          }
          if (audit.missingA11yNames.length > 0) {
            throw new Error(`Missing accessible names at ${theme}/${locale}/${vp.name}: ${audit.missingA11yNames.join(", ")}`);
          }
          if (audit.openTechDetailsCount > 0 || audit.anyTechCodeVisibleOutsideOpen.length > 0) {
            throw new Error(`Technical IDs exposed by default at ${theme}/${locale}/${vp.name}`);
          }
          if (audit.contrastFailures.length > 0) {
            throw new Error(`Contrast failures at ${theme}/${locale}/${vp.name}: ${JSON.stringify(audit.contrastFailures)}`);
          }

          const filename = `gallery-${theme}-${locale}-${vp.name}.png`;
          await page.screenshot({ path: path.join(outDir, filename), fullPage: true });
          results.push({ filename, theme, locale, viewport: vp, audit });
          await context.close();
        }
      }
    }

    // Interactive state & keyboard focus-cycle checks (light, en, 1280x840)
    const context = await browser.newContext({ viewport: { width: 1280, height: 840 } });
    const page = await context.newPage();
    await page.goto(`${baseUrl}?theme=light&locale=en`, { waitUntil: "networkidle" });

    const getActiveInfo = () =>
      page.evaluate(() => {
        const el = document.activeElement;
        return {
          tag: el?.tagName ?? "",
          label: el?.getAttribute("aria-label") || el?.textContent?.trim() || "",
          inDialog: Boolean(el?.closest('[role="dialog"][aria-modal="true"]')),
          inAside: Boolean(el?.closest(".ui2-aside")),
        };
      });

    // Dialog keyboard open, Tab / Shift+Tab wrap, and Escape restore
    const dialogOpener = page.getByRole("button", { name: "Open modal dialog" });
    await dialogOpener.focus();
    await page.keyboard.press("Enter");
    await page.waitForSelector('[role="dialog"][aria-modal="true"]:not([hidden])');
    const dialogInit = await getActiveInfo();
    if (!dialogInit.inDialog || dialogInit.label !== "Close") {
      throw new Error(`Dialog initial focus unexpected: ${JSON.stringify(dialogInit)}`);
    }
    await page.screenshot({ path: path.join(outDir, "gallery-state-dialog-light-en-wide.png") });
    await page.keyboard.press("Tab");
    const dialogCancel = await getActiveInfo();
    await page.keyboard.press("Tab");
    const dialogApprove = await getActiveInfo();
    await page.keyboard.press("Tab");
    const dialogWrappedForward = await getActiveInfo();
    if (
      dialogCancel.label !== "Cancel" ||
      dialogApprove.label !== "Approve" ||
      !dialogWrappedForward.inDialog ||
      dialogWrappedForward.label !== "Close"
    ) {
      throw new Error(
        `Dialog forward Tab trap failed: ${JSON.stringify({ dialogCancel, dialogApprove, dialogWrappedForward })}`
      );
    }
    await page.keyboard.press("Shift+Tab");
    const dialogWrappedBackward = await getActiveInfo();
    if (!dialogWrappedBackward.inDialog || dialogWrappedBackward.label !== "Approve") {
      throw new Error(`Dialog Shift+Tab trap failed: ${JSON.stringify(dialogWrappedBackward)}`);
    }
    await page.keyboard.press("Escape");
    await page.waitForSelector('[role="dialog"][aria-modal="true"]', { state: "hidden" });
    const dialogAfterEscape = await getActiveInfo();
    if (dialogAfterEscape.label !== "Open modal dialog") {
      throw new Error(`Dialog Escape focus restore failed: ${JSON.stringify(dialogAfterEscape)}`);
    }

    // Aside keyboard open, Tab / Shift+Tab wrap, and Escape restore
    const asideOpener = page.getByRole("button", { name: "Open detail drawer" });
    await asideOpener.focus();
    await page.keyboard.press("Enter");
    await page.waitForSelector(".ui2-aside:not([hidden])");
    const asideInit = await getActiveInfo();
    if (!asideInit.inAside || asideInit.label !== "Close") {
      throw new Error(`Aside initial focus unexpected: ${JSON.stringify(asideInit)}`);
    }
    await page.screenshot({ path: path.join(outDir, "gallery-state-aside-light-en-wide.png"), fullPage: true });
    await page.keyboard.press("Tab");
    const asideOpenChat = await getActiveInfo();
    await page.keyboard.press("Tab");
    const asideSummary = await getActiveInfo();
    await page.keyboard.press("Tab");
    const asideWrappedForward = await getActiveInfo();
    if (
      asideOpenChat.label !== "Open chat" ||
      !asideSummary.inAside ||
      asideSummary.tag !== "SUMMARY" ||
      !asideWrappedForward.inAside ||
      asideWrappedForward.label !== "Close"
    ) {
      throw new Error(
        `Aside forward Tab trap failed: ${JSON.stringify({ asideOpenChat, asideSummary, asideWrappedForward })}`
      );
    }
    await page.keyboard.press("Shift+Tab");
    const asideWrappedBackward = await getActiveInfo();
    if (!asideWrappedBackward.inAside || asideWrappedBackward.tag !== "SUMMARY") {
      throw new Error(`Aside Shift+Tab trap failed: ${JSON.stringify(asideWrappedBackward)}`);
    }
    await page.keyboard.press("Escape");
    await page.waitForSelector(".ui2-aside", { state: "hidden" });
    const asideAfterEscape = await getActiveInfo();
    if (asideAfterEscape.label !== "Open detail drawer") {
      throw new Error(`Aside Escape focus restore failed: ${JSON.stringify(asideAfterEscape)}`);
    }

    const keyboardFocusCycle = {
      dialog: {
        wrappedForward: dialogWrappedForward.label,
        wrappedBackward: dialogWrappedBackward.label,
        restoredOpener: dialogAfterEscape.label,
      },
      aside: {
        visitedSummary: `${asideSummary.tag}:${asideSummary.label}`,
        wrappedForward: asideWrappedForward.label,
        wrappedBackward: `${asideWrappedBackward.tag}:${asideWrappedBackward.label}`,
        restoredOpener: asideAfterEscape.label,
      },
    };

    // Technical details open (unambiguous Section 4 selector)
    await page.locator('.ui2-card > .ui2-stack > [data-ui2-technical-details="true"] > summary').click();
    const openAudit = await runAuditAndChecks(page);
    if (openAudit.contrastFailures.length > 0 || openAudit.openTechDetailsCount !== 1) {
      throw new Error(`Details open check failed: ${JSON.stringify(openAudit)}`);
    }
    await page.screenshot({ path: path.join(outDir, "gallery-state-details-light-en-wide.png"), fullPage: true });

    await context.close();

    // Pseudo-locale overflow / a11y / contrast stress matrix across light+dark & wide/laptop/narrow
    const pseudoResults = [];
    for (const theme of ["light", "dark"]) {
      for (const vp of viewports) {
        const pseudoCtx = await browser.newContext({
          viewport: { width: vp.width, height: vp.height },
        });
        const pseudoPage = await pseudoCtx.newPage();
        await pseudoPage.goto(`${baseUrl}?theme=${theme}&locale=pseudo`, {
          waitUntil: "networkidle",
        });
        await pseudoPage.waitForSelector(".ui2-root");

        const pseudoAudit = await runAuditAndChecks(pseudoPage);
        if (pseudoAudit.overflowPx > 0) {
          throw new Error(
            `Pseudo-locale horizontal overflow (${pseudoAudit.overflowPx}px) at ${theme}/${vp.name}`
          );
        }
        if (pseudoAudit.missingA11yNames.length > 0) {
          throw new Error(
            `Pseudo-locale missing accessible names at ${theme}/${vp.name}: ${pseudoAudit.missingA11yNames.join(", ")}`
          );
        }
        if (pseudoAudit.contrastFailures.length > 0) {
          throw new Error(
            `Pseudo-locale contrast failures at ${theme}/${vp.name}: ${JSON.stringify(pseudoAudit.contrastFailures)}`
          );
        }

        const filename = `gallery-state-pseudo-${theme}-${vp.name}.png`;
        await pseudoPage.screenshot({ path: path.join(outDir, filename), fullPage: true });
        pseudoResults.push({ filename, theme, locale: "pseudo", viewport: vp, audit: pseudoAudit });
        await pseudoCtx.close();
      }
    }

    const interactiveStates = [
      "gallery-state-dialog-light-en-wide.png",
      "gallery-state-aside-light-en-wide.png",
      "gallery-state-details-light-en-wide.png",
      ...pseudoResults.map((r) => r.filename),
    ];

    const manifest = {
      generatedAt: new Date().toISOString(),
      totalCaptures: results.length + interactiveStates.length,
      coreMatrix: results,
      pseudoMatrix: pseudoResults,
      keyboardFocusCycle,
      interactiveStates,
    };
    fs.writeFileSync(path.join(outDir, "gallery-manifest.json"), JSON.stringify(manifest, null, 2));
    console.log(
      `UI2 gallery visual/a11y OK — ${manifest.totalCaptures} captures, minContrast>=${Math.min(
        ...results.map((r) => r.audit.minContrast),
        ...pseudoResults.map((r) => r.audit.minContrast)
      )}, overflow=0px`
    );
  } finally {
    await browser.close();
    await viteServer.close();
  }
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});

// MP-7MCP-02: visual evidence for the "Choose chats" dialog. Renders the real
// dialog (real UI2 tokens and primitives) in Chromium with a mocked native
// layer and synthetic chats. Output: docs/ui/evidence/mp-7mcp-02/choose-chats/.
// Usage: node scripts/ui2-choose-chats-visual.mjs [outDir]
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";
import { createServer } from "vite";

const rootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const outDir = process.argv[2]
  ? path.resolve(process.argv[2])
  : path.join(rootDir, "docs", "ui", "evidence", "mp-7mcp-02", "choose-chats");

const PAGE_HTML = `<!DOCTYPE html>
<html lang="en" data-theme="light" data-density="comfortable">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>Choose chats preview</title>
  <link rel="stylesheet" href="/src/ui2/ui-kit/tokens.css" />
  <link rel="stylesheet" href="/src/ui2/ui-kit/primitives.css" />
</head>
<body>
  <main class="ui2-root"><button id="opener" class="ui2-btn">Choose chats</button></main>
  <script type="module">
    import { createI18nController } from "/src/ui2/i18n/index.ts";
    import { ensureUi2IconSprite } from "/src/ui2/ui-kit/icons.ts";
    import { openChooseChatsDialog } from "/src/ui2/connections/choose-chats-dialog.ts";

    const params = new URLSearchParams(window.location.search);
    document.documentElement.setAttribute("data-theme", params.get("theme") || "light");
    const i18n = createI18nController({
      initialLocale: params.get("locale") || "en",
      storage: { getItem: () => null, setItem: () => {} },
    });
    ensureUi2IconSprite(document);
    const hex = (n) => "src:v1:" + n.toString(16).padStart(64, "0");
    const titles = [
      ["Weekly planning for the launch", "chatgpt", true],
      ["Notes on backup and restore design", "claude", true],
      ["Trip ideas for December", "gemini", false],
      ["Why the sidebar flickers on high-DPI screens and what we tried before settling on a fix", "chatgpt", false],
      ["Imported: meeting transcript", "generic", false],
      ["Recipe swaps", "claude", false],
      ["Budget review", "chatgpt", true],
      ["Reading list", "gemini", false],
    ];
    const chats = titles.map(([title, provider, granted], i) => ({
      source_id: hex(i + 1), title, provider, granted,
      captured_at: "2026-10-" + String(10 + i).padStart(2, "0") + "T10:00:00Z",
    }));
    const invoke = async (command, args = {}) => {
      if (command === "list_local_mcp_grantable_chats") {
        const q = String(args.query || "").toLowerCase();
        const rows = chats.filter((c) => c.title.toLowerCase().includes(q));
        return { total: rows.length, allowed: rows.filter((c) => c.granted).length, rows };
      }
      return 0;
    };
    window.__dialog = openChooseChatsDialog({
      doc: document, i18n, invoke,
      client: { id: "client:v1:preview", displayName: "Claude Code" },
      opener: document.getElementById("opener"),
    });
  </script>
</body>
</html>`;

const previewPlugin = {
  name: "choose-chats-preview",
  configureServer(server) {
    server.middlewares.use(async (req, res, next) => {
      if ((req.url || "").split("?")[0] === "/__choose-chats.html") {
        const html = await server.transformIndexHtml(req.url, PAGE_HTML);
        res.statusCode = 200;
        res.setHeader("Content-Type", "text/html; charset=utf-8");
        res.end(html);
        return;
      }
      next();
    });
  },
};

const viewports = [
  { name: "wide", width: 1440, height: 900 },
  { name: "laptop", width: 1024, height: 720 },
  { name: "narrow", width: 390, height: 844 },
];

fs.mkdirSync(outDir, { recursive: true });
const server = await createServer({
  root: rootDir,
  plugins: [previewPlugin],
  server: { host: "127.0.0.1", port: 0, strictPort: false },
  logLevel: "error",
});
await server.listen();
const { port } = server.httpServer.address();
const base = `http://127.0.0.1:${port}/__choose-chats.html`;
// CHROMIUM_PATH lets a machine with a slightly different cached Chromium build
// run this without downloading another browser.
const browser = await chromium.launch({
  headless: true,
  ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}),
});
const written = [];
try {
  for (const theme of ["light", "dark"]) {
    for (const locale of ["en", "id"]) {
      for (const vp of viewports) {
        const context = await browser.newContext({ viewport: { width: vp.width, height: vp.height } });
        const page = await context.newPage();
        const problems = [];
        page.on("pageerror", (error) => problems.push(String(error)));
        page.on("console", (message) => {
          // The preview page has no favicon; that 404 is not a defect.
          if (message.type() === "error" && !message.location().url.endsWith("/favicon.ico")) {
            problems.push(message.text());
          }
        });
        await page.goto(`${base}?theme=${theme}&locale=${locale}`, { waitUntil: "load", timeout: 120000 });
        await page.waitForSelector("[role=switch]", { timeout: 60000 });
        const choose = `choose-${theme}-${locale}-${vp.name}.png`;
        await page.screenshot({ path: path.join(outDir, choose) });
        written.push(choose);
        // Review step at the laptop width: flip one allowed and one blocked chat.
        if (vp.name === "laptop") {
          const switches = page.locator("[role=switch]");
          await switches.nth(0).click();
          await switches.nth(2).click();
          await page.locator(".ui2-dialog .ui2-btn-primary").click();
          const review = `review-${theme}-${locale}-${vp.name}.png`;
          await page.screenshot({ path: path.join(outDir, review) });
          written.push(review);
        }
        if (problems.length > 0) {
          throw new Error(`Browser errors for ${theme}/${locale}/${vp.name}: ${problems.join(" | ")}`);
        }
        await context.close();
      }
    }
  }
} finally {
  await browser.close();
  await server.close();
}
console.log(JSON.stringify({ outDir, files: written }, null, 2));

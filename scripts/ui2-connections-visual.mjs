// MP-UI2-09: visual evidence for the Connections page, rendered inside the real
// UI2 shell (real tokens, primitives, shell CSS) with a mocked native layer and
// synthetic data. Output: docs/ui/evidence/mp-ui2-09/connections/.
// Usage: node scripts/ui2-connections-visual.mjs [outDir]  (CHROMIUM_PATH optional)
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";
import { createServer } from "vite";

const rootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const outDir = process.argv[2]
  ? path.resolve(process.argv[2])
  : path.join(rootDir, "docs", "ui", "evidence", "mp-ui2-09", "connections");

const PAGE_HTML = `<!DOCTYPE html>
<html lang="en" data-theme="light" data-density="comfortable">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>Connections preview</title>
  <link rel="stylesheet" href="/src/ui2/ui-kit/tokens.css" />
  <link rel="stylesheet" href="/src/ui2/ui-kit/primitives.css" />
  <link rel="stylesheet" href="/src/ui2/shell/shell.css" />
  <link rel="stylesheet" href="/src/ui2/connections/connections.css" />
</head>
<body>
  <div id="root"></div>
  <script type="module">
    import { createI18nController } from "/src/ui2/i18n/index.ts";
    import { mountUi2Shell } from "/src/ui2/shell/shell.ts";
    import { createUi2Pages } from "/src/ui2/shell/pages.ts";

    const params = new URLSearchParams(window.location.search);
    document.documentElement.setAttribute("data-theme", params.get("theme") || "light");
    const hex = (n) => "src:v1:" + n.toString(16).padStart(64, "0");
    const clients = [
      { id: "client:v1:1", display_name: "Claude Code", state: "enabled", policy_revision: 2 },
      { id: "client:v1:2", display_name: "Notes helper", state: "enabled", policy_revision: 1 },
      { id: "client:v1:3", display_name: "Old experiment", state: "revoked", policy_revision: 4 },
    ];
    const granted = { "client:v1:1": [hex(1), hex(2), hex(3)], "client:v1:2": [] };
    const chats = ["Weekly planning for the launch", "Notes on backup and restore design", "Trip ideas for December", "Budget review"]
      .map((title, i) => ({ source_id: hex(i + 1), title, provider: ["chatgpt", "claude", "gemini", "chatgpt"][i], captured_at: "2026-10-1" + i + "T10:00:00Z", granted: i < 2 }));
    let running = true;
    let registered = ["chrome"];
    const invoke = async (command, args = {}) => {
      switch (command) {
        case "get_local_mcp_status": return { running, endpoint: "x" };
        case "start_local_mcp": running = true; return { running, endpoint: "x" };
        case "stop_local_mcp": running = false; return { running, endpoint: "x" };
        case "list_local_mcp_clients": return clients;
        case "list_local_mcp_client_sources": return granted[args.clientId] || [];
        case "issue_local_mcp_reader": {
          clients.splice(1, 0, { id: "client:v1:9", display_name: args.displayName, state: "enabled", policy_revision: 1 });
          granted["client:v1:9"] = [];
          return { client_id: "client:v1:9", display_name: args.displayName, credential: "mpc_v1_" + "ab12".repeat(16), endpoint: "x" };
        }
        case "get_hosted_tunnel_status": return { running: false, tunnel_id: null, connection_verified: false };
        case "get_setup": return { registered_browsers: registered };
        case "register_extension": registered = [...registered, args.browser]; return { registered_browsers: registered };
        case "list_local_mcp_grantable_chats": return { total: chats.length, allowed: chats.filter((c) => c.granted).length, rows: chats };
        default: return undefined;
      }
    };
    const i18n = createI18nController({
      initialLocale: params.get("locale") || "en",
      storage: { getItem: () => null, setItem: () => {} },
    });
    window.location.hash = "#ui2=" + encodeURIComponent("/connections");
    mountUi2Shell(document.getElementById("root"), {
      onLock: () => {},
      i18n,
      pages: createUi2Pages({ invoke, clipboard: { writeText: async () => {} } }),
    });
  </script>
</body>
</html>`;

const previewPlugin = {
  name: "connections-preview",
  configureServer(server) {
    server.middlewares.use(async (req, res, next) => {
      if ((req.url || "").split("?")[0] === "/__connections.html") {
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
  // No file watching or HMR: a one-shot render must not crawl target/ and release/.
  server: { host: "127.0.0.1", port: 0, strictPort: false, watch: null, hmr: false },
  // The preview imports only dependency-free UI2 modules. Scanning every HTML
  // file in the repo (build output, fixtures) would take minutes for nothing.
  optimizeDeps: { entries: [], noDiscovery: true },
  logLevel: "error",
});
await server.listen();
const { port } = server.httpServer.address();
const base = `http://127.0.0.1:${port}/__connections.html`;
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
          if (message.type() === "error" && !message.location().url.endsWith("/favicon.ico")) {
            problems.push(message.text());
          }
        });
        const pending = new Set();
        page.on("request", (request) => pending.add(request.url()));
        page.on("requestfinished", (request) => pending.delete(request.url()));
        page.on("requestfailed", (request) => pending.delete(request.url()));
        try {
          await page.goto(`${base}?theme=${theme}&locale=${locale}`, { waitUntil: "domcontentloaded", timeout: 120000 });
        } catch (error) {
          throw new Error(`Page did not load. Pending requests: ${[...pending].join(", ")}. ${error}`);
        }
        await page.waitForSelector(".ui2-list-card .ui2-tag", { timeout: 60000 });
        const file = `connections-${theme}-${locale}-${vp.name}.png`;
        await page.screenshot({ path: path.join(outDir, file), fullPage: true });
        written.push(file);
        if (vp.name === "laptop") {
          // State: a key was just issued (shown once).
          await page.getByRole("button", { name: /Create access|Buat akses/ }).click();
          await page.waitForSelector(".ui2-tech-code", { timeout: 15000 });
          const issued = `connections-key-${theme}-${locale}-${vp.name}.png`;
          await page.screenshot({ path: path.join(outDir, issued), fullPage: true });
          written.push(issued);
          // State: inline two-step revoke.
          await page.locator(".ui2-list-row .ui2-btn-danger").first().click();
          const revoke = `connections-revoke-${theme}-${locale}-${vp.name}.png`;
          await page.screenshot({ path: path.join(outDir, revoke), fullPage: true });
          written.push(revoke);
          // State: the collapsed Advanced disclosure opened.
          await page.locator("summary", { hasText: /Advanced|Lanjutan/ }).click();
          const advanced = `connections-advanced-${theme}-${locale}-${vp.name}.png`;
          await page.screenshot({ path: path.join(outDir, advanced), fullPage: true });
          written.push(advanced);
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

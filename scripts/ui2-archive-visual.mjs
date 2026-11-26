// MP-UI2-06: visual evidence for the Archive page and chat reader, rendered inside
// the real UI2 shell (real tokens, primitives, shell CSS) with a mocked native layer
// and synthetic data only. Output: docs/ui/evidence/mp-ui2-06/.
// Usage: node scripts/ui2-archive-visual.mjs [outDir]  (CHROMIUM_PATH optional)
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";
import { createServer } from "vite";

const rootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const outDir = process.argv[2]
  ? path.resolve(process.argv[2])
  : path.join(rootDir, "docs", "ui", "evidence", "mp-ui2-06");

const PAGE_HTML = `<!DOCTYPE html>
<html lang="en" data-theme="light" data-density="comfortable">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>Archive preview</title>
  <link rel="stylesheet" href="/src/ui2/ui-kit/tokens.css" />
  <link rel="stylesheet" href="/src/ui2/ui-kit/primitives.css" />
  <link rel="stylesheet" href="/src/ui2/shell/shell.css" />
  <link rel="stylesheet" href="/src/ui2/chats/chats.css" />
</head>
<body>
  <div id="root"></div>
  <script type="module">
    import { createI18nController } from "/src/ui2/i18n/index.ts";
    import { mountUi2Shell } from "/src/ui2/shell/shell.ts";
    import { createUi2Pages } from "/src/ui2/shell/pages.ts";

    const params = new URLSearchParams(window.location.search);
    document.documentElement.setAttribute("data-theme", params.get("theme") || "light");
    const state = params.get("state") || "list";
    const hex = (n) => "src:v1:" + n.toString(16).padStart(64, "0");
    const snap = (n) => n.toString(16).padStart(64, "0");

    const canvas = document.createElement("canvas");
    canvas.width = 480; canvas.height = 200;
    const g = canvas.getContext("2d");
    g.fillStyle = "#d4d4d8"; g.fillRect(0, 0, 480, 200);
    g.fillStyle = "#71717a"; g.fillRect(24, 24, 200, 24); g.fillRect(24, 64, 340, 16); g.fillRect(24, 96, 280, 16);
    const picture = canvas.toDataURL("image/png");

    const sample = [
      ["Mind-place save testing", "claude", "2026-10-24", true],
      ["Capture host: Rust or Go?", "chatgpt", "2026-11-09", false],
      ["K-Means clustering study notes", "chatgpt", "2026-11-02", false],
      ["Designing a calm interface", "claude", "2026-10-30", false],
      ["EEG preprocessing pipeline", "chatgpt", "2026-10-26", false],
      ["Weekly planning between classes and the app", "gemini", "2026-10-21", false],
    ];
    const summaries = [];
    for (let i = 0; i < 62; i += 1) {
      const base = sample[i % sample.length];
      summaries.push({
        id: hex(i + 1), conversation_id: "synthetic-" + i, provider: base[1],
        title: i < sample.length ? base[0] : base[0] + " (" + (i + 1) + ")",
        source_url: "https://example.test/chat/" + i, conversation_date: base[2] + "T10:00:00Z", date_source: "provider-updated",
        captured_at: base[2] + "T10:00:00Z", message_count: 4, snapshot_count: i === 0 ? 3 : 1,
        completeness: base[3] && i < sample.length ? "partial" : "complete", warnings: [],
      });
    }
    const first = summaries[0];
    const message = (i, role, text, extra = {}) => ({ id: "m" + i, role, text, format: "markdown", attachments: [], ...extra });
    const build = (messages, snapshotId) => ({
      summary: first,
      snapshot_id: snapshotId,
      capture: {
        schema_version: 1, provider: "claude", conversation_id: first.conversation_id, source_url: "https://claude.ai/chat/synthetic",
        title: first.title, captured_at: first.captured_at, capture_method: "manual-dom", completeness: "partial", warnings: ["the page showed part of the chat"], messages,
      },
    });
    const longAnswer = [
      "Claude responded: Got it, this chat is set aside for your mind-place save testing. I do not have memory of past chats turned on here, so each conversation starts fresh for me.",
      "What would you like to try first? For example:\\n\\n- send me some sample content to save\\n- tell me what the feature should do and I will help you write test cases\\n- paste an error and we can work through it",
      "A tiny checklist you could start with:\\n\\n\`\`\`text\\n1. Save a short chat\\n2. Save a long chat\\n3. Save a chat with a picture\\n\`\`\`",
      "just now",
    ].join("\\n\\n");
    const messages = [
      message(1, "user", "this chat is for mind-place save testing"),
      message(2, "assistant", longAnswer),
      message(3, "user", "Here is the layout I meant, can you describe what you see?"),
      message(4, "assistant", "I can see a card with a title and two short lines of text below it. It looks tidy.", {
        attachments: [{ id: "a1", name: "layout.png", mime_type: "image/png", asset_path: "assets/layout.png" }],
      }),
    ];
    const latest = build(messages, snap(3));
    const middle = build(messages.slice(0, 3), snap(2));
    const snapshots = [
      { snapshot_id: snap(3), captured_at: "2026-10-24T10:00:00Z", message_count: 4, completeness: "partial" },
      { snapshot_id: snap(2), captured_at: "2026-10-23T10:00:00Z", message_count: 3, completeness: "partial" },
      { snapshot_id: snap(1), captured_at: "2026-10-22T10:00:00Z", message_count: 1, completeness: "partial" },
    ];
    const invoke = async (command, args = {}) => {
      switch (command) {
        case "list_conversations": return state === "empty" ? [] : summaries;
        case "read_conversation": return args.snapshotId === snap(2) ? middle : latest;
        case "list_snapshots": return snapshots;
        case "get_asset": return { mime_type: "image/png", data_url: picture };
        case "export_markdown": return { path: "", markdown: "# Synthetic export" };
        default: return undefined;
      }
    };
    const i18n = createI18nController({
      initialLocale: params.get("locale") || "en",
      storage: { getItem: () => null, setItem: () => {} },
    });
    window.location.hash = "#ui2=" + encodeURIComponent("/archive");
    mountUi2Shell(document.getElementById("root"), {
      onLock: () => {},
      i18n,
      pages: createUi2Pages({ invoke, clipboard: { writeText: async () => {} } }),
    });
  </script>
</body>
</html>`;

const previewPlugin = {
  name: "archive-preview",
  configureServer(server) {
    server.middlewares.use(async (req, res, next) => {
      if ((req.url || "").split("?")[0] === "/__archive.html") {
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
  optimizeDeps: { entries: [], noDiscovery: true },
  logLevel: "error",
});
await server.listen();
const { port } = server.httpServer.address();
const base = `http://127.0.0.1:${port}/__archive.html`;
const browser = await chromium.launch({
  headless: true,
  ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}),
});
const written = [];
try {
  for (const theme of ["light", "dark"]) {
    for (const locale of ["en", "id"]) {
      for (const vp of viewports) {
        const context = await browser.newContext({ viewport: { width: vp.width, height: vp.height }, reducedMotion: "reduce" });
        const page = await context.newPage();
        const problems = [];
        page.on("pageerror", (error) => problems.push(String(error)));
        page.on("console", (message) => {
          if (message.type() === "error" && !message.location().url.endsWith("/favicon.ico")) problems.push(message.text());
        });
        await page.goto(`${base}?theme=${theme}&locale=${locale}`, { waitUntil: "domcontentloaded", timeout: 180000 });
        await page.waitForSelector("button.ui2-list-row", { timeout: 120000 });
        const shot = async (name, fullPage = true) => {
          const file = `${name}-${theme}-${locale}-${vp.name}.png`;
          await page.screenshot({ path: path.join(outDir, file), fullPage });
          written.push(file);
        };
        await shot("archive", false);
        // The chat reader (a partial copy, with a picture and provider labels in the raw text).
        await page.locator("button.ui2-list-row").first().click();
        await page.waitForSelector("img.ui2-chats-image", { timeout: 30000 });
        await shot("chat");
        // Info aside: grouped metadata, saved copies, completeness, collapsed technical details.
        await page.getByRole("button", { name: /^(Info)$/ }).click();
        await page.waitForSelector(".ui2-chats-aside:not([hidden])");
        await shot("info", false);
        if (vp.name === "laptop") {
          await page.keyboard.press("Escape");
          await page.getByRole("button", { name: /^(Export|Ekspor)$/ }).click();
          await page.waitForSelector(".ui2-chats-aside:not([hidden])");
          await page.locator(".ui2-chats-aside:not([hidden]) button.ui2-btn-primary").click();
          await page.waitForSelector(".ui2-chats-aside:not([hidden]) .ui2-row-flex button");
          await shot("export", false);
        }
        if (problems.length > 0) throw new Error(`Browser errors for ${theme}/${locale}/${vp.name}: ${problems.join(" | ")}`);
        await context.close();
      }
      // Empty archive (laptop only).
      const context = await browser.newContext({ viewport: { width: 1024, height: 720 }, reducedMotion: "reduce" });
      const page = await context.newPage();
      await page.goto(`${base}?theme=${theme}&locale=${locale}&state=empty`, { waitUntil: "domcontentloaded", timeout: 180000 });
      await page.waitForSelector(".ui2-empty", { timeout: 120000 });
      const file = `archive-empty-${theme}-${locale}-laptop.png`;
      await page.screenshot({ path: path.join(outDir, file), fullPage: true });
      written.push(file);
      await context.close();
    }
  }
} finally {
  await browser.close();
  await server.close();
}
console.log(JSON.stringify({ outDir, files: written }, null, 2));

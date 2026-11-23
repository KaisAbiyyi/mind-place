// MP-UI2-09: automated keyboard / accessibility pass for the Connections page.
// Real UI2 shell, real tokens and primitives, mocked native layer, synthetic data,
// driven ONLY with the keyboard in Chromium. This is NOT a screen-reader test and
// NOT a WebView2 run: those stay owner-only manual gates (see the receipt).
// Usage: node scripts/ui2-connections-a11y.mjs [outDir]
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";
import { createServer } from "vite";

const rootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const outDir = process.argv[2]
  ? path.resolve(process.argv[2])
  : path.join(rootDir, "docs", "ui", "evidence", "mp-ui2-09", "a11y");

const PAGE_HTML = `<!DOCTYPE html>
<html lang="en" data-theme="light" data-density="comfortable">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>Connections a11y</title>
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
    const hex = (n) => "src:v1:" + n.toString(16).padStart(64, "0");
    const clients = [
      { id: "client:v1:1", display_name: "Claude Code", state: "enabled", policy_revision: 2 },
      { id: "client:v1:2", display_name: "Notes helper", state: "enabled", policy_revision: 1 },
    ];
    const chats = ["Weekly planning", "Backup design", "Trip ideas"].map((title, i) => ({
      source_id: hex(i + 1), title, provider: "chatgpt", captured_at: "2026-10-1" + i + "T10:00:00Z", granted: i < 1,
    }));
    let running = false;
    window.__copied = [];
    window.__locked = 0;
    const invoke = async (command, args = {}) => {
      switch (command) {
        case "get_local_mcp_status": return { running, endpoint: "x" };
        case "start_local_mcp": running = true; return { running, endpoint: "x" };
        case "stop_local_mcp": running = false; return { running, endpoint: "x" };
        case "list_local_mcp_clients": return clients.map((c) => ({ ...c }));
        case "list_local_mcp_client_sources": return [];
        case "issue_local_mcp_reader":
          clients.push({ id: "client:v1:9", display_name: args.displayName, state: "enabled", policy_revision: 1 });
          return { client_id: "client:v1:9", display_name: args.displayName, credential: "mpc_v1_" + "ab12".repeat(16), endpoint: "x" };
        case "revoke_local_mcp_client": clients.find((c) => c.id === args.clientId).state = "revoked"; return undefined;
        case "get_hosted_tunnel_status": return { running: false, tunnel_id: null, connection_verified: false };
        case "get_setup": return { registered_browsers: ["chrome"] };
        case "register_extension": return { registered_browsers: ["chrome", args.browser] };
        case "list_local_mcp_grantable_chats": return { total: chats.length, allowed: chats.filter((c) => c.granted).length, rows: chats };
        default: return undefined;
      }
    };
    const params = new URLSearchParams(window.location.search);
    const clipboard = params.get("clipboard") === "none" ? undefined : { writeText: async (v) => { window.__copied.push(v.length); } };
    const i18n = createI18nController({ initialLocale: "en", storage: { getItem: () => null, setItem: () => {} } });
    window.location.hash = "#ui2=" + encodeURIComponent("/connections");
    mountUi2Shell(document.getElementById("root"), {
      onLock: () => { window.__locked += 1; },
      i18n,
      pages: createUi2Pages({ invoke, ...(clipboard ? { clipboard } : {}) }),
    });
  </script>
</body>
</html>`;

const previewPlugin = {
  name: "connections-a11y",
  configureServer(server) {
    server.middlewares.use(async (req, res, next) => {
      if ((req.url || "").split("?")[0] === "/__a11y.html") {
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

fs.mkdirSync(outDir, { recursive: true });
const server = await createServer({
  root: rootDir,
  plugins: [previewPlugin],
  server: {
    host: "127.0.0.1", port: 0, strictPort: false, hmr: false,
    // Never crawl build output (a concurrent cargo build locks files there).
    watch: { ignored: ["**/target/**", "**/release/**", "**/outputs/**", "**/node_modules/**", "**/.git/**"] },
  },
  optimizeDeps: { entries: [], noDiscovery: true },
  logLevel: "error",
});
await server.listen();
const base = `http://127.0.0.1:${server.httpServer.address().port}/__a11y.html`;
const browser = await chromium.launch({
  headless: true,
  ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}),
});

const checks = [];
const guarded = async (id, fn) => {
  try { await fn(); } catch (error) { checks.push({ id, title: "check did not complete", result: "FAIL", detail: String(error.message ?? error).split(/\r?\n/)[0].slice(0, 300) }); }
};
const record = (id, title, ok, detail) => checks.push({ id, title, result: ok ? "PASS" : "FAIL", detail: String(detail ?? "").slice(0, 300) });

/** What currently has focus, described without reading any secret. */
const focusInfo = (page) =>
  page.evaluate(() => {
    const el = document.activeElement;
    if (!el || el === document.body) return { lost: true };
    const labelled = el.labels && el.labels[0] ? el.labels[0].textContent : "";
    const label = el.getAttribute("aria-label") || labelled || (el.textContent || "").trim().replace(/\s+/g, " ").slice(0, 40) || el.tagName;
    const rect = el.getBoundingClientRect();
    const style = getComputedStyle(el);
    const focusVisible = el.matches(":focus-visible");
    const ring = (style.outlineStyle !== "none" && parseFloat(style.outlineWidth) > 0) || (style.boxShadow && style.boxShadow !== "none");
    return { lost: false, tag: el.tagName, role: el.getAttribute("role"), label, w: Math.round(rect.width), h: Math.round(rect.height), focusVisible, ring: Boolean(ring) };
  });

async function openPage(extra = "", viewport = { width: 1024, height: 720 }) {
  const context = await browser.newContext({ viewport });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto(`${base}${extra}`, { waitUntil: "domcontentloaded", timeout: 120000 });
  await page.waitForSelector(".ui2-list-card .ui2-tag", { timeout: 60000 });
  return { context, page, errors };
}

try {
  // K1 + K9: Tab order reaches every control, each has a name and a visible focus ring.
  await guarded("K1", async () => {
    const { context, page, errors } = await openPage();
    const stops = [];
    await page.evaluate(() => document.body.focus());
    for (let i = 0; i < 60; i += 1) {
      await page.keyboard.press("Tab");
      const info = await focusInfo(page);
      if (info.lost) break;
      if (stops.some((s) => s.label === info.label && s.tag === info.tag && s.n === i - 1 && false)) break;
      stops.push({ ...info, n: i });
      if (stops.length > 1 && stops[0].label === info.label && stops[0].tag === info.tag && i > 3) break;
    }
    const labels = stops.map((s) => s.label);
    const need = [/Share memory with AI apps/, /Create access/, /Choose chats/, /Revoke access/, /Advanced/, /Register/];
    const missing = need.filter((rx) => !labels.some((l) => rx.test(l)));
    record("K1", "Tab reaches the sharing switch, Create access, Choose chats, Revoke, Register and the Advanced disclosure", missing.length === 0, `stops=${labels.length}; missing=${missing.join(",")}`);
    record("K1b", "Every tab stop has an accessible name", stops.every((s) => s.label && s.label !== "BUTTON" && s.label !== "INPUT"), stops.filter((s) => !s.label || s.label === "BUTTON" || s.label === "INPUT").map((s) => s.tag).join(","));
    const noRing = stops.filter((s) => !s.ring && !s.focusVisible);
    record("K9", "Every tab stop shows a visible focus indicator", noRing.length === 0, noRing.map((s) => s.label).join(" | "));
    record("K1c", "No uncaught page errors during the tab walk", errors.length === 0, errors.join(" | "));
    await context.close();
  });

  // K2: the sharing switch works with Space and Enter-free toggling, and keeps focus.
  await guarded("K2", async () => {
    const { context, page } = await openPage();
    const sw = page.getByRole("switch", { name: "Share memory with AI apps" });
    await sw.focus();
    await page.keyboard.press("Space");
    await page.getByText("Sharing is on").first().waitFor();
    await page.waitForTimeout(150);
    const on = await sw.getAttribute("aria-checked");
    const afterToggle = await focusInfo(page);
    record("K2b", "Focus stays on the sharing switch after it is toggled from the keyboard", !afterToggle.lost && /Share memory/.test(afterToggle.label ?? ""), JSON.stringify(afterToggle));
    await sw.focus();
    await page.keyboard.press("Space");
    await page.getByText("Sharing is off").first().waitFor();
    const off = await sw.getAttribute("aria-checked");
    record("K2", "Space toggles the sharing switch both ways, state exposed via aria-checked", on === "true" && off === "false", `on=${on} off=${off}`);
    await context.close();
  });

  // K3 + K3b: create access and copy with the keyboard only; the key is a named element.
  await guarded("K3", async () => {
    const { context, page } = await openPage();
    await page.getByRole("button", { name: "Create access" }).focus();
    await page.keyboard.press("Enter");
    await page.waitForSelector(".ui2-tech-code");
    const named = await page.locator(".ui2-tech-code").first().getAttribute("aria-label");
    const copy = page.getByRole("button", { name: "Copy" });
    await copy.focus();
    await page.keyboard.press("Enter");
    await page.getByRole("button", { name: "Copied" }).waitFor();
    const copied = await page.evaluate(() => window.__copied.length);
    record("K3", "Create access and Copy work with Enter; the one-time key has an accessible name", Boolean(named) && copied === 1, `name=${named}; copiedWrites=${copied}`);
    await context.close();
  });

  // K3c: without a clipboard the Copy control is disabled and says why (no false success).
  await guarded("K3", async () => {
    const { context, page } = await openPage("?clipboard=none");
    await page.getByRole("button", { name: "Create access" }).focus();
    await page.keyboard.press("Enter");
    await page.waitForSelector(".ui2-tech-code");
    const disabled = await page.getByRole("button", { name: "Copy" }).isDisabled();
    const hint = await page.getByText(/copy it yourself/).count();
    const copied = await page.getByRole("button", { name: "Copied" }).count();
    record("K3c", "Without a clipboard, Copy is disabled, a hint explains manual copy, and nothing says Copied", disabled && hint === 1 && copied === 0, `disabled=${disabled} hint=${hint} copied=${copied}`);
    await context.close();
  });

  // K4: Choose chats dialog: focus enters, is trapped, Escape closes, focus returns to the opener.
  await guarded("K4", async () => {
    const { context, page } = await openPage();
    const opener = page.getByRole("button", { name: "Choose chats" }).first();
    await opener.focus();
    await page.keyboard.press("Enter");
    await page.getByText(/Choose chats for/).waitFor();
    await page.waitForTimeout(150);
    const inside = () => page.evaluate(() => Boolean(document.activeElement?.closest("[role=dialog], dialog, .ui2-dialog")));
    const entered = await inside();
    let trapped = true;
    for (let i = 0; i < 14; i += 1) { await page.keyboard.press("Tab"); if (!(await inside())) trapped = false; }
    for (let i = 0; i < 6; i += 1) { await page.keyboard.press("Shift+Tab"); if (!(await inside())) trapped = false; }
    await page.keyboard.press("Escape");
    await page.waitForTimeout(150);
    const closed = (await page.getByText(/Choose chats for/).count()) === 0;
    const back = await page.evaluate(() => document.activeElement?.textContent?.trim());
    record("K4", "Choose chats: focus moves into the dialog, Tab/Shift+Tab stay inside, Escape closes it, focus returns to the opener", entered && trapped && closed && back === "Choose chats", `entered=${entered} trapped=${trapped} closed=${closed} focusAfter=${back}`);
    await context.close();
  });

  // K5: inline two-step revoke with the keyboard; focus is never lost.
  await guarded("K5", async () => {
    const { context, page } = await openPage();
    const trigger = page.locator(".ui2-conn-app").first().getByRole("button", { name: "Revoke access" });
    await trigger.focus();
    await page.keyboard.press("Enter");
    await page.getByText(/stops working right away/).filter({ visible: true }).waitFor();
    const afterOpen = await focusInfo(page);
    await page.keyboard.press("Tab");
    const next = await focusInfo(page);
    const cancel = page.locator(".ui2-inline-confirm-box").getByRole("button", { name: "Cancel" });
    await cancel.focus();
    await page.keyboard.press("Enter");
    await page.waitForTimeout(100);
    const gone = (await page.getByText(/stops working right away/).filter({ visible: true }).count()) === 0;
    const afterCancel = await focusInfo(page);
    record("K5", "Inline revoke opens from the keyboard, Cancel closes it without revoking, and focus is never dropped to the page body", !afterOpen.lost && !next.lost && gone && !afterCancel.lost, `afterOpen=${afterOpen.label} next=${next.label} cancelClosed=${gone} afterCancel=${afterCancel.label}`);
    await page.locator(".ui2-conn-app").first().getByRole("button", { name: "Revoke access" }).focus();
    await page.keyboard.press("Enter");
    await page.locator(".ui2-inline-confirm-box").getByRole("button", { name: "Revoke access" }).focus();
    await page.keyboard.press("Enter");
    await page.getByText(/Revoked apps/).waitFor();
    const afterRevoke = await focusInfo(page);
    record("K5b", "Confirming revoke from the keyboard moves the app to Revoked apps and focus is not lost", !afterRevoke.lost, `focusAfter=${afterRevoke.label ?? "BODY"}`);
    await context.close();
  });

  // K6: Advanced disclosure and the hosted consent control.
  await guarded("K6", async () => {
    const { context, page } = await openPage();
    const summary = page.locator("summary", { hasText: /Advanced/ });
    await summary.focus();
    await page.keyboard.press("Enter");
    const open1 = await page.locator("details", { has: summary }).first().evaluate((d) => d.open);
    await page.keyboard.press("Space");
    const open2 = await page.locator("details", { has: summary }).first().evaluate((d) => d.open);
    await page.keyboard.press("Space");
    const open3 = await page.locator("details", { has: summary }).first().evaluate((d) => d.open);
    const hasConsent = await page.getByRole("switch", { name: /understand|remote|ChatGPT|memory/i }).count();
    record("K6", "The Advanced disclosure opens and closes with Enter and Space, and the hosted consent is a labelled switch", open1 === true && open2 === false && open3 === true && hasConsent >= 1, `afterEnter=${open1} afterSpace=${open2} reopened=${open3} checkboxes=${hasConsent}`);
    await context.close();
  });

  // K7: language switch by keyboard keeps the page and focus usable.
  await guarded("K7", async () => {
    const { context, page } = await openPage();
    const id = page.locator("[data-ui2-control=language]").getByText("ID", { exact: true });
    await id.focus();
    await page.keyboard.press("Enter");
    await page.getByText("Buat akses").first().waitFor();
    const info = await focusInfo(page);
    record("K7", "EN to ID by keyboard swaps the page copy without losing focus", !info.lost, `focus=${info.label ?? "BODY"}`);
    await context.close();
  });

  // K8: narrow viewport: no horizontal scroll, controls at least 24x24 CSS px (WCAG 2.2 target size minimum).
  await guarded("K8", async () => {
    const { context, page } = await openPage("", { width: 390, height: 844 });
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
    const small = await page.evaluate(() =>
      [...document.querySelectorAll("button, summary, [role=switch], input")]
        .filter((el) => el.getClientRects().length > 0)
        .map((el) => ({ el: (el.getAttribute("aria-label") || el.textContent || el.tagName).trim().slice(0, 30), w: el.getBoundingClientRect().width, h: el.getBoundingClientRect().height }))
        .filter((r) => r.w < 24 || r.h < 24));
    record("K8", "At 390px width there is no horizontal scrolling", overflow <= 1, `overflowPx=${overflow}`);
    checks.push({ id: "K8b", title: "Controls smaller than 24x24 CSS px (WCAG 2.2 target-size minimum; the spacing exception was not evaluated)", result: small.length === 0 ? "PASS" : "NOTE", detail: JSON.stringify(small).slice(0, 250) });
    await context.close();
  });

  // K11: typing into fields from the keyboard keeps focus and the typed value (no focus theft by re-render).
  await guarded("K11", async () => {
    const { context, page } = await openPage();
    const name = page.getByLabel("Name of the app");
    await name.focus();
    await page.keyboard.type("Keyboard app", { delay: 15 });
    await page.waitForTimeout(250);
    const info = await focusInfo(page);
    const value = await name.inputValue();
    record("K11", "Typing the app name keeps focus and the typed text", !info.lost && value === "Keyboard app", `value=${value} focus=${info.label ?? "BODY"}`);
    await context.close();
  });

  // K12: Create access, Revoke confirm and Choose chats leave focus on a sensible control, never the page body.
  await guarded("K12", async () => {
    const { context, page } = await openPage();
    await page.getByRole("button", { name: "Create access" }).focus();
    await page.keyboard.press("Enter");
    await page.waitForSelector(".ui2-tech-code");
    await page.waitForTimeout(250);
    const afterCreate = await focusInfo(page);
    record("K12", "After Create access from the keyboard, focus is not dropped to the page body", !afterCreate.lost, `focus=${afterCreate.label ?? "BODY"}`);
    await context.close();
  });

  // K10: Lock control is reachable by keyboard and invokes the shell lock handler.
  await guarded("K10", async () => {
    const { context, page } = await openPage();
    const lock = page.getByRole("button", { name: "Lock" }).first();
    await lock.focus();
    await page.keyboard.press("Enter");
    const locked = await page.evaluate(() => window.__locked);
    record("K10", "Lock is reachable and activates with Enter", locked === 1, `lockCalls=${locked}`);
    await context.close();
  });
} finally {
  await browser.close();
  await server.close();
}

const receipt = {
  kind: "automated keyboard-only pass in Chromium against the real UI2 shell with a mocked native layer",
  not_covered: [
    "a screen reader (NVDA, Narrator or JAWS) reading the page and dialogs",
    "the packaged app in real WebView2 (focus rings, IME, high-DPI, Windows high contrast)",
    "the owner's own keyboard-only walk-through (owner gate)",
  ],
  checks,
  verdict: checks.every((c) => c.result === "PASS" || c.result === "NOTE") ? "PASS" : "FAIL",
  notes_for_owner: checks.filter((c) => c.result === "NOTE").map((c) => c.id + ": " + c.title),
};
fs.writeFileSync(path.join(outDir, "receipt.json"), JSON.stringify(receipt, null, 2) + "\n");
console.log(JSON.stringify({ verdict: receipt.verdict, checks: checks.map((c) => `${c.result} ${c.id} ${c.title}${c.result === "FAIL" ? " :: " + c.detail : ""}`) }, null, 2));
process.exitCode = receipt.verdict === "PASS" ? 0 : 1;

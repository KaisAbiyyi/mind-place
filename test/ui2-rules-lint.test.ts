import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";
import { verifyUiRules } from "../scripts/verify-ui-rules.mjs";

const REPO_ROOT = path.resolve(import.meta.dirname, "..");

test("MP-UI2-03: Live src/ui2 codebase passes all UI rules cleanly", () => {
  const result = verifyUiRules({ rootDir: REPO_ROOT });
  assert.equal(result.ok, true, JSON.stringify(result.violations, null, 2));
  assert.ok(result.checkedFiles >= 10);
});

test("MP-UI2-03: Linter rejects raw colors, off-scale sizes, raw strings, parity drift, and exposed technical IDs", () => {
  // 1. Raw color in non-token CSS and TS
  const rawColorResult = verifyUiRules({
    checkParity: false,
    files: [
      { path: "src/ui2/views/bad.css", content: ".x { color: #ff0000; background: rgba(0, 0, 0, 0.5); }" },
      { path: "src/ui2/views/bad.ts", content: "const c = '#123456';" },
    ],
  });
  assert.equal(rawColorResult.ok, false);
  assert.ok(rawColorResult.violations.some((v) => v.code === "MP_UI2_RULE_RAW_COLOR"));

  // 2. Off-scale font-size, weight, spacing
  const scaleResult = verifyUiRules({
    checkParity: false,
    files: [
      {
        path: "src/ui2/views/scale.css",
        content: ".box { font-size: 15px; font-weight: 700; padding: 15px; }",
      },
    ],
  });
  assert.equal(scaleResult.ok, false);
  assert.equal(scaleResult.violations.filter((v) => v.code === "MP_UI2_RULE_FONT_AND_SPACE_SCALE").length, 3);

  // 3. Raw strings & window.confirm
  const rawStringResult = verifyUiRules({
    checkParity: false,
    files: [
      {
        path: "src/ui2/views/home.ts",
        content: `
          export function bad(el: HTMLElement) {
            if (window.confirm("Sure?")) {
              el.textContent = "Deleted permanently";
              el.setAttribute("aria-label", "Hardcoded label");
            }
          }
        `,
      },
    ],
  });
  assert.equal(rawStringResult.ok, false);
  assert.equal(rawStringResult.violations.filter((v) => v.code === "MP_UI2_RULE_RAW_STRING").length, 3);

  // 4. i18n parity & placeholder & product-name drift
  const parityResult = verifyUiRules({
    files: [
      {
        path: "src/ui2/i18n/catalog-en.ts",
        content: `export const EN_CATALOG = {
          "a.missing": "Hello {name}",
          "a.ph": "Count {count}",
          "a.brand": "Connect ChatGPT"
        };`,
      },
      {
        path: "src/ui2/i18n/catalog-id.ts",
        content: `export const ID_CATALOG = {
          "a.ph": "Jumlah {wrong}",
          "a.brand": "Hubungkan Obrolan",
          "a.extra": "Extra key"
        };`,
      },
    ],
  });
  assert.equal(parityResult.ok, false);
  assert.equal(parityResult.violations.filter((v) => v.code === "MP_UI2_RULE_I18N_PARITY").length, 4);

  // 5. Raw technical IDs outside Details vs allowed inside createUi2TechnicalDetails
  const leakedTechResult = verifyUiRules({
    checkParity: false,
    files: [
      {
        path: "src/ui2/views/leak.ts",
        content: `export const x = { sha: "sha256:abcdef1234", code: "MP_VAULT_NOT_READY", url: "http://127.0.0.1:43731/mcp" };`,
      },
    ],
  });
  assert.equal(leakedTechResult.ok, false);
  assert.equal(leakedTechResult.violations.filter((v) => v.code === "MP_UI2_RULE_TECH_IDENTIFIER").length, 3);

  const tuckedTechResult = verifyUiRules({
    checkParity: false,
    files: [
      {
        path: "src/ui2/views/safe.ts",
        content: `
          export function makeSafe(doc: Document) {
            return createUi2TechnicalDetails(doc, {
              summaryLabel: "Details",
              entries: [{ label: "Digest", value: "sha256:abcdef1234 · rev_0042 · MP_VAULT_READY" }],
            });
          }
        `,
      },
    ],
  });
  assert.equal(tuckedTechResult.ok, true, JSON.stringify(tuckedTechResult.violations));
});

test("UI2 rules linter catches raw copy in component options, helper calls, and ternary expressions", () => {
  const result = verifyUiRules({
    checkParity: false,
    files: [
      {
        path: "src/ui2/views/blindspots.ts",
        content: `
          export function badView(doc: Document, el: HTMLElement, toast: { show(m: string): void }, field: { setError(m: string): void }, cond: boolean) {
            createUi2Button(doc, { label: "Save changes", ariaLabel: "Save item", variant: "primary" });
            createUi2Field(doc, { label: "Name", placeholder: "Type name", hint: "Short hint", error: "Invalid name" });
            createUi2SettingsCard(doc, {
              title: "General",
              rows: [{ id: "r1", title: "Row title", description: "Row description" }],
            });
            createUi2Dialog(doc, { title: "Delete folder?", closeLabel: "Dismiss" });
            createUi2StatCard(doc, { label: i18n.t("stat.chats"), value: "42 chats" });
            toast.show("Saved");
            field.setError("Required");
            createUi2LoadingSkeleton(doc, "Loading items");
            createUi2Button(doc, { label: cond ? "Raw label" : i18n.t("action.save") });
            el.ariaLabel = "Raw aria label";
            el.append("Raw appended text");
            el.textContent = cond ? "Raw" : i18n.t("ok");
          }
        `,
      },
    ],
  });

  assert.equal(result.ok, false);
  const rawViolations = result.violations.filter((v) => v.code === "MP_UI2_RULE_RAW_STRING");
  assert.equal(rawViolations.length, 19, JSON.stringify(rawViolations, null, 2));
});

test("UI2 rules linter allows internal tokens, i18n keys, DOM helpers, and equality comparisons", () => {
  const result = verifyUiRules({
    checkParity: false,
    files: [
      {
        path: "src/ui2/views/clean.ts",
        content: `
          export function goodView(doc: Document, el: HTMLElement, locale: string, i18n: { t(k: string): string }) {
            createUi2Button(doc, { label: i18n.t("action.save"), variant: "primary", size: "md", icon: "search" });
            createUi2Segmented(doc, {
              ariaLabel: i18n.t("lang.label"),
              value: "en",
              options: [{ value: "en", label: i18n.t("lang.en") }],
            });
            createUi2Disclosure(doc, {
              triggerLabel: i18n.t("details.show"),
              body: doc.createElement("div"),
            });
            el.setAttribute("aria-hidden", "true");
            return { text: locale === "pseudo" ? i18n.t("a") : i18n.t("b") };
          }
        `,
      },
    ],
  });

  assert.equal(result.ok, true, JSON.stringify(result.violations, null, 2));
});

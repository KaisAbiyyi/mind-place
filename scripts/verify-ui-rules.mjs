import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ALLOWED_FONT_SIZES_PX = new Set([12, 13, 14, 16, 20, 28]);
const ALLOWED_FONT_WEIGHTS = new Set(["400", "500", "600"]);
const ALLOWED_SPACING_PX = new Set([0, 1, 2, 4, 8, 12, 16, 24, 32, 48]);

const PROTECTED_PRODUCT_NAMES = [
  "Mind Place",
  "ChatGPT",
  "Claude Desktop",
  "Claude",
  "Gemini",
  "MCP",
  "Chrome",
  "Edge",
  "Brave",
  "Markdown",
  "JSON",
  "HTML",
  "SHA-256",
  "SQLite",
];

const TECH_IDENTIFIER_PATTERNS = [
  /sha256:[0-9a-fA-F]{6,}/,
  /\b(?:MP|E)_[A-Z0-9_]{3,}\b/,
  /\brev_[0-9a-fA-F]{3,}\b/,
  /https?:\/\/127\.0\.0\.1(?::\d+)?/,
];

function collectFiles(dir, out = []) {
  if (!fs.existsSync(dir)) return out;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      collectFiles(full, out);
    } else if (entry.isFile() && /\.(css|ts|tsx|mjs|js)$/.test(entry.name)) {
      out.push(full);
    }
  }
  return out;
}

function normalizeRel(filePath) {
  return filePath.replace(/\\/g, "/");
}

function lineNumberAt(text, pos) {
  let line = 1;
  for (let i = 0; i < pos && i < text.length; i += 1) {
    if (text[i] === "\n") line += 1;
  }
  return line;
}

function stripCssComments(source) {
  return source.replace(/\/\*[\s\S]*?\*\//g, (match) => match.replace(/[^\n]/g, " "));
}

/**
 * Masks comments and extracts string/template literals from TypeScript/JS source
 * using zero external dependencies (works even before npm install).
 */
function scanScriptLiterals(source) {
  let maskedCode = "";
  const literals = [];
  const n = source.length;
  let i = 0;

  while (i < n) {
    const ch = source[i];
    const next = source[i + 1];

    // Single-line comment
    if (ch === "/" && next === "/") {
      const start = i;
      i += 2;
      while (i < n && source[i] !== "\n") i += 1;
      maskedCode += " ".repeat(i - start);
      continue;
    }

    // Multi-line comment
    if (ch === "/" && next === "*") {
      const start = i;
      i += 2;
      while (i < n && !(source[i] === "*" && source[i + 1] === "/")) i += 1;
      if (i < n) i += 2;
      maskedCode += source.slice(start, i).replace(/[^\n]/g, " ");
      continue;
    }

    // String or template literal
    if (ch === '"' || ch === "'" || ch === "`") {
      const quote = ch;
      const start = i;
      i += 1;
      let rawValue = "";
      let templateStaticOnly = "";
      while (i < n) {
        const c = source[i];
        if (c === "\\") {
          const esc = source[i + 1] ?? "";
          rawValue += c + esc;
          templateStaticOnly += esc;
          i += 2;
          continue;
        }
        if (quote === "`" && c === "$" && source[i + 1] === "{") {
          // Skip template interpolation depth-wise
          i += 2;
          let depth = 1;
          while (i < n && depth > 0) {
            if (source[i] === "{") depth += 1;
            else if (source[i] === "}") depth -= 1;
            i += 1;
          }
          templateStaticOnly += " ";
          continue;
        }
        if (c === quote) {
          i += 1;
          break;
        }
        rawValue += c;
        templateStaticOnly += c;
        i += 1;
      }
      literals.push({
        start,
        end: i,
        quote,
        raw: rawValue,
        staticText: quote === "`" ? templateStaticOnly : decodeBasicEscapes(rawValue),
      });
      // Replace literal contents in maskedCode with spaces preserving length/newlines
      const inner = source.slice(start, i);
      maskedCode += inner.replace(/[^\n]/g, " ");
      continue;
    }

    maskedCode += ch;
    i += 1;
  }

  return { maskedCode, literals };
}

function decodeBasicEscapes(value) {
  return value
    .replace(/\\n/g, "\n")
    .replace(/\\r/g, "\r")
    .replace(/\\t/g, "\t")
    .replace(/\\(["'`\\])/g, "$1")
    .replace(/\\u([0-9a-fA-F]{4})/g, (_, hex) => String.fromCharCode(parseInt(hex, 16)));
}

function extractPlaceholders(value) {
  return [...new Set(value.match(/\{[a-zA-Z0-9_]+\}/g) ?? [])].sort();
}

function extractCatalogMap(source) {
  const { literals } = scanScriptLiterals(source);
  const map = new Map();
  for (let idx = 0; idx < literals.length - 1; idx += 1) {
    const keyLit = literals[idx];
    const valLit = literals[idx + 1];
    const between = source.slice(keyLit.end, valLit.start).trim();
    if (between === ":") {
      map.set(keyLit.staticText, valLit.staticText);
      idx += 1;
    }
  }
  return map;
}

/**
 * Checks if character position `pos` falls inside an argument list of
 * `createUi2TechnicalDetails(...)`, `renderUi2TechnicalCode(...)`, or `new Error(...)`.
 */
function isInsideAllowedTechnicalContext(source, pos) {
  const callRegex = /\b(?:createUi2TechnicalDetails|renderUi2TechnicalCode|new\s+Error)\s*\(/g;
  for (const match of source.matchAll(callRegex)) {
    const openParen = (match.index ?? 0) + match[0].length - 1;
    if (pos <= openParen) continue;
    let depth = 0;
    let inStr = null;
    for (let i = openParen; i < source.length; i += 1) {
      const c = source[i];
      if (inStr) {
        if (c === "\\") {
          i += 1;
        } else if (c === inStr) {
          inStr = null;
        }
        continue;
      }
      if (c === '"' || c === "'" || c === "`") {
        inStr = c;
        continue;
      }
      if (c === "(") depth += 1;
      else if (c === ")") {
        depth -= 1;
        if (depth === 0) {
          if (pos > openParen && pos < i) return true;
          break;
        }
      }
    }
  }
  return false;
}

function checkCssFile(relPath, content, violations) {
  const clean = stripCssComments(content);
  const isTokenCss = relPath.endsWith("src/ui2/ui-kit/tokens.css");
  if (isTokenCss) return;

  // Rule 1: No raw colors outside tokens.css
  for (const m of clean.matchAll(/#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})\b/g)) {
    violations.push({
      code: "MP_UI2_RULE_RAW_COLOR",
      file: relPath,
      line: lineNumberAt(clean, m.index ?? 0),
      detail: `Raw hex color "${m[0]}" is forbidden outside tokens.css; use a var(--*) token.`,
    });
  }
  for (const m of clean.matchAll(/\b(?:rgba?|hsla?)\s*\(/gi)) {
    violations.push({
      code: "MP_UI2_RULE_RAW_COLOR",
      file: relPath,
      line: lineNumberAt(clean, m.index ?? 0),
      detail: `Raw color function "${m[0]}" is forbidden outside tokens.css; use a var(--*) token.`,
    });
  }

  // Rule 2: Typography & spacing scale outside tokens.css
  for (const m of clean.matchAll(/font-size\s*:\s*([^;}{]+)/gi)) {
    const value = m[1].trim();
    if (/^var\(--t-[a-z]+\)$/i.test(value) || value === "inherit") continue;
    const px = value.match(/^(\d+(?:\.\d+)?)px$/i);
    if (px && ALLOWED_FONT_SIZES_PX.has(Number(px[1]))) continue;
    violations.push({
      code: "MP_UI2_RULE_FONT_AND_SPACE_SCALE",
      file: relPath,
      line: lineNumberAt(clean, m.index ?? 0),
      detail: `Off-scale font-size "${value}"; must use var(--t-*) or [12, 13, 14, 16, 20, 28]px.`,
    });
  }

  for (const m of clean.matchAll(/font-weight\s*:\s*([^;}{]+)/gi)) {
    const value = m[1].trim();
    if (/^var\(--w-[a-z]+\)$/i.test(value) || value === "inherit" || ALLOWED_FONT_WEIGHTS.has(value)) {
      continue;
    }
    violations.push({
      code: "MP_UI2_RULE_FONT_AND_SPACE_SCALE",
      file: relPath,
      line: lineNumberAt(clean, m.index ?? 0),
      detail: `Off-scale font-weight "${value}"; must be var(--w-*) or one of 400, 500, 600.`,
    });
  }

  for (const m of clean.matchAll(/(?:^|[;\s{])(?:padding|margin|gap|row-gap|column-gap)(?:-[a-z-]+)?\s*:\s*([^;}{]+)/gi)) {
    const value = m[1].trim();
    for (const pxMatch of value.matchAll(/(-?\d+(?:\.\d+)?)px\b/gi)) {
      const pxVal = Number(pxMatch[1]);
      if (!ALLOWED_SPACING_PX.has(pxVal)) {
        violations.push({
          code: "MP_UI2_RULE_FONT_AND_SPACE_SCALE",
          file: relPath,
          line: lineNumberAt(clean, m.index ?? 0),
          detail: `Off-scale spacing "${pxMatch[0]}" in "${value}"; use var(--s*) or canonical px values.`,
        });
      }
    }
  }
}

function checkTsFile(relPath, content, violations) {
  const isTokenTs = relPath.endsWith("src/ui2/ui-kit/tokens.ts");
  const isCatalog =
    relPath.endsWith("src/ui2/i18n/catalog-en.ts") || relPath.endsWith("src/ui2/i18n/catalog-id.ts");
  const isIcons = relPath.endsWith("src/ui2/ui-kit/icons.ts");
  const isI18nCore = relPath.startsWith("src/ui2/i18n/") || relPath.includes("/src/ui2/i18n/");

  const { maskedCode, literals } = scanScriptLiterals(content);

  // Rule 1: Raw colors in string/template literals outside tokens.ts
  if (!isTokenTs) {
    for (const lit of literals) {
      if (/#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})\b/.test(lit.raw)) {
        violations.push({
          code: "MP_UI2_RULE_RAW_COLOR",
          file: relPath,
          line: lineNumberAt(content, lit.start),
          detail: `Raw hex color inside literal is forbidden outside tokens.ts.`,
        });
      }
      if (/\b(?:rgba?|hsla?)\s*\(/i.test(lit.raw)) {
        violations.push({
          code: "MP_UI2_RULE_RAW_COLOR",
          file: relPath,
          line: lineNumberAt(content, lit.start),
          detail: `Raw color function inside literal is forbidden outside tokens.ts.`,
        });
      }
    }
  }

  // Rule 3a: Forbid native window.confirm / alert / prompt
  for (const m of maskedCode.matchAll(/(?:^|[^\w$.])(?:(?:window|globalThis)\.)?(confirm|alert|prompt)\s*\(/g)) {
    const pos = m.index ?? 0;
    // Skip function/method definitions if any unlikely pattern matched
    const before = maskedCode.slice(Math.max(0, pos - 16), pos);
    if (/\bfunction\s+$/.test(before)) continue;
    violations.push({
      code: "MP_UI2_RULE_RAW_STRING",
      file: relPath,
      line: lineNumberAt(content, pos),
      detail: `Native ${m[1]}() calls are forbidden; use createUi2InlineConfirm or createUi2Dialog.`,
    });
  }

  // Rule 3b & Rule 3c: Forbid raw UI copy assigned to DOM text/attributes
  if (!isCatalog && !isIcons) {
    for (const lit of literals) {
      if (!/[A-Za-z]{2,}/.test(lit.staticText)) continue;
      const before = maskedCode.slice(Math.max(0, lit.start - 48), lit.start);
      if (/\.(?:textContent|innerText|placeholder|title|innerHTML)\s*=\s*$/.test(before)) {
        violations.push({
          code: "MP_UI2_RULE_RAW_STRING",
          file: relPath,
          line: lineNumberAt(content, lit.start),
          detail: `Raw UI string "${lit.staticText.trim()}" assigned directly to DOM property; use i18n.t(...).`,
        });
        continue;
      }
      const attrBefore = content.slice(Math.max(0, lit.start - 64), lit.start);
      if (/\.setAttribute\(\s*["'](?:aria-label|title|placeholder)["']\s*,\s*$/.test(attrBefore)) {
        violations.push({
          code: "MP_UI2_RULE_RAW_STRING",
          file: relPath,
          line: lineNumberAt(content, lit.start),
          detail: `Raw literal "${lit.staticText.trim()}" passed to setAttribute(); use i18n.t(...).`,
        });
      }
    }
  }

  // Rule 5: Technical identifiers outside Details / Error helpers
  if (!isI18nCore) {
    for (const lit of literals) {
      if (TECH_IDENTIFIER_PATTERNS.some((rx) => rx.test(lit.staticText))) {
        if (!isInsideAllowedTechnicalContext(content, lit.start)) {
          violations.push({
            code: "MP_UI2_RULE_TECH_IDENTIFIER",
            file: relPath,
            line: lineNumberAt(content, lit.start),
            detail: `Technical identifier "${lit.staticText}" must not be rendered outside createUi2TechnicalDetails / renderUi2TechnicalCode or Errors.`,
          });
        }
      }
    }
  }
}

export function verifyUiRules(options = {}) {
  const rootDir = options.rootDir
    ? path.resolve(options.rootDir)
    : path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  const violations = [];

  let entries;
  if (Array.isArray(options.files)) {
    entries = options.files.map((f) => ({
      relPath: normalizeRel(f.path),
      content: f.content,
    }));
  } else {
    const ui2Root = path.join(rootDir, "src", "ui2");
    entries = collectFiles(ui2Root).map((abs) => ({
      relPath: normalizeRel(path.relative(rootDir, abs)),
      content: fs.readFileSync(abs, "utf8"),
    }));
  }

  for (const entry of entries) {
    if (entry.relPath.endsWith(".css")) {
      checkCssFile(entry.relPath, entry.content, violations);
    } else if (entry.relPath.endsWith(".ts") || entry.relPath.endsWith(".tsx")) {
      checkTsFile(entry.relPath, entry.content, violations);
    }
  }

  // Rule 4: EN/ID catalog parity check
  if (options.checkParity !== false) {
    const enEntry = entries.find((e) => e.relPath.endsWith("src/ui2/i18n/catalog-en.ts"));
    const idEntry = entries.find((e) => e.relPath.endsWith("src/ui2/i18n/catalog-id.ts"));
    if (enEntry && idEntry) {
      const enMap = extractCatalogMap(enEntry.content);
      const idMap = extractCatalogMap(idEntry.content);
      for (const key of enMap.keys()) {
        if (!idMap.has(key)) {
          violations.push({
            code: "MP_UI2_RULE_I18N_PARITY",
            file: idEntry.relPath,
            line: 1,
            detail: `Missing Indonesian translation key "${key}".`,
          });
          continue;
        }
        const enPh = extractPlaceholders(enMap.get(key));
        const idPh = extractPlaceholders(idMap.get(key));
        if (enPh.join(",") !== idPh.join(",")) {
          violations.push({
            code: "MP_UI2_RULE_I18N_PARITY",
            file: idEntry.relPath,
            line: 1,
            detail: `Placeholder mismatch for "${key}": EN=[${enPh.join(", ")}] vs ID=[${idPh.join(", ")}].`,
          });
        }
        for (const name of PROTECTED_PRODUCT_NAMES) {
          if (enMap.get(key).includes(name) && !idMap.get(key).includes(name)) {
            violations.push({
              code: "MP_UI2_RULE_I18N_PARITY",
              file: idEntry.relPath,
              line: 1,
              detail: `Protected product name "${name}" was altered or dropped in Indonesian key "${key}".`,
            });
          }
        }
      }
      for (const key of idMap.keys()) {
        if (!enMap.has(key)) {
          violations.push({
            code: "MP_UI2_RULE_I18N_PARITY",
            file: idEntry.relPath,
            line: 1,
            detail: `Extra Indonesian translation key "${key}" has no English counterpart.`,
          });
        }
      }
    }
  }

  return {
    ok: violations.length === 0,
    checkedFiles: entries.length,
    violations,
  };
}

const isCli =
  process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url;

if (isCli) {
  const result = verifyUiRules();
  if (!result.ok) {
    for (const v of result.violations) {
      console.error(`${v.code} ${v.file}:${v.line} — ${v.detail}`);
    }
    process.exitCode = 1;
  } else {
    console.log(`UI2 design rules OK — ${result.checkedFiles} files verified`);
  }
}

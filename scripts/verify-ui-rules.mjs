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
  "Constellation",
];

const UI_COPY_OPTION_KEYS = new Set([
  "label",
  "ariaLabel",
  "title",
  "subtitle",
  "placeholder",
  "hint",
  "error",
  "meta",
  "statement",
  "translatedFromLabel",
  "toggleOriginalLabel",
  "actionLabel",
  "body",
  "message",
  "triggerLabel",
  "question",
  "confirmLabel",
  "cancelLabel",
  "closeLabel",
  "summaryLabel",
  "description",
  "roleLabel",
  "timestampLabel",
  "text",
]);

const DOM_UI_ATTRS = new Set(["aria-label", "title", "placeholder", "alt"]);

const REGEX_BEFORE_KEYWORDS = new Set([
  "return",
  "throw",
  "case",
  "default",
  "typeof",
  "void",
  "delete",
  "in",
  "instanceof",
  "new",
  "else",
  "yield",
  "await",
  "export",
  "of",
  "do",
]);

const GROUPING_PREFIX_KEYWORDS = new Set([
  "return",
  "throw",
  "typeof",
  "void",
  "delete",
  "await",
  "yield",
  "in",
  "instanceof",
  "case",
  "of",
  "else",
  "do",
]);

const MATCHING_DELIM = { "(": ")", "[": "]", "{": "}" };

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
  const n = source.length;
  const maskedChars = new Array(n);
  for (let idx = 0; idx < n; idx += 1) {
    maskedChars[idx] = source[idx];
  }
  const literals = [];
  let canStartRegex = true;

  function maskSpan(start, end) {
    for (let k = start; k < end && k < n; k += 1) {
      if (maskedChars[k] !== "\n") {
        maskedChars[k] = " ";
      }
    }
  }

  function scanRange(pos, stopAtCloseBrace) {
    let braceDepth = 0;
    while (pos < n) {
      const ch = source[pos];
      const next = source[pos + 1];

      if (stopAtCloseBrace) {
        if (ch === "{") {
          braceDepth += 1;
        } else if (ch === "}") {
          if (braceDepth === 0) {
            return pos;
          }
          braceDepth -= 1;
        }
      }

      // Single-line comment
      if (ch === "/" && next === "/") {
        const start = pos;
        pos += 2;
        while (pos < n && source[pos] !== "\n") pos += 1;
        maskSpan(start, pos);
        continue;
      }

      // Multi-line comment
      if (ch === "/" && next === "*") {
        const start = pos;
        pos += 2;
        while (pos < n && !(source[pos] === "*" && source[pos + 1] === "/")) pos += 1;
        if (pos < n) pos += 2;
        maskSpan(start, pos);
        continue;
      }

      // Regex literal in expression-start position
      if (ch === "/" && canStartRegex && next !== "/" && next !== "*") {
        const start = pos;
        pos += 1;
        let inClass = false;
        while (pos < n) {
          const c = source[pos];
          if (c === "\\") {
            pos += 2;
            continue;
          }
          if (c === "\n") break;
          if (c === "[") {
            inClass = true;
          } else if (c === "]" && inClass) {
            inClass = false;
          } else if (c === "/" && !inClass) {
            pos += 1;
            break;
          }
          pos += 1;
        }
        while (pos < n && /[a-zA-Z]/.test(source[pos])) pos += 1;
        maskSpan(start, pos);
        canStartRegex = false;
        continue;
      }

      // Quoted string literal
      if (ch === '"' || ch === "'") {
        const quote = ch;
        const start = pos;
        pos += 1;
        let rawValue = "";
        while (pos < n) {
          const c = source[pos];
          if (c === "\\") {
            const esc = source[pos + 1] ?? "";
            rawValue += c + esc;
            pos += 2;
            continue;
          }
          if (c === quote) {
            pos += 1;
            break;
          }
          rawValue += c;
          pos += 1;
        }
        literals.push({
          start,
          end: pos,
          quote,
          raw: rawValue,
          staticText: decodeBasicEscapes(rawValue),
        });
        maskSpan(start, pos);
        canStartRegex = false;
        continue;
      }

      // Template literal (recursively scans ${...} interpolations)
      if (ch === "`") {
        const start = pos;
        maskSpan(pos, pos + 1);
        pos += 1;
        let rawValue = "";
        let templateStaticOnly = "";
        while (pos < n) {
          const c = source[pos];
          if (c === "\\") {
            const esc = source[pos + 1] ?? "";
            rawValue += c + esc;
            templateStaticOnly += decodeBasicEscapes(c + esc);
            maskSpan(pos, Math.min(n, pos + 2));
            pos += 2;
            continue;
          }
          if (c === "$" && source[pos + 1] === "{") {
            maskSpan(pos, pos + 2);
            pos += 2;
            const savedCanStartRegex = canStartRegex;
            canStartRegex = true;
            const closeIdx = scanRange(pos, true);
            canStartRegex = savedCanStartRegex;
            rawValue += "${}";
            templateStaticOnly += " ";
            if (closeIdx < n) {
              maskSpan(closeIdx, closeIdx + 1);
              pos = closeIdx + 1;
            } else {
              pos = closeIdx;
            }
            continue;
          }
          if (c === "`") {
            maskSpan(pos, pos + 1);
            pos += 1;
            break;
          }
          rawValue += c;
          templateStaticOnly += c;
          maskSpan(pos, pos + 1);
          pos += 1;
        }
        literals.push({
          start,
          end: pos,
          quote: "`",
          raw: rawValue,
          staticText: templateStaticOnly,
        });
        canStartRegex = false;
        continue;
      }

      if (/[A-Za-z_$]/.test(ch)) {
        const idStart = pos;
        while (pos < n && /[A-Za-z0-9_$]/.test(source[pos])) pos += 1;
        const ident = source.slice(idStart, pos);
        let prev = idStart - 1;
        while (prev >= 0 && /\s/.test(maskedChars[prev])) prev -= 1;
        const isPropName = prev >= 0 && maskedChars[prev] === ".";
        canStartRegex = !isPropName && REGEX_BEFORE_KEYWORDS.has(ident);
        continue;
      }

      if (/[0-9]/.test(ch)) {
        while (pos < n && /[0-9a-fA-FxXbBoO_n.]/.test(source[pos])) pos += 1;
        canStartRegex = false;
        continue;
      }

      if (/\s/.test(ch)) {
        pos += 1;
        continue;
      }

      if ((ch === "+" && next === "+") || (ch === "-" && next === "-")) {
        pos += 2;
        canStartRegex = false;
        continue;
      }

      if (ch === ")" || ch === "]" || ch === ".") {
        canStartRegex = false;
      } else {
        canStartRegex = true;
      }
      pos += 1;
    }
    return pos;
  }

  scanRange(0, false);
  literals.sort((a, b) => a.start - b.start);
  return { maskedCode: maskedChars.join(""), literals };
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

function findMatchingDelimiter(maskedCode, openPos) {
  const opener = maskedCode[openPos];
  if (!MATCHING_DELIM[opener]) return -1;
  const stack = [];
  for (let i = openPos; i < maskedCode.length; i += 1) {
    const ch = maskedCode[i];
    if (ch === "(" || ch === "[" || ch === "{") {
      stack.push(MATCHING_DELIM[ch]);
    } else if (ch === ")" || ch === "]" || ch === "}") {
      if (stack.length > 0 && stack[stack.length - 1] === ch) {
        stack.pop();
        if (stack.length === 0) return i;
      }
    }
  }
  return -1;
}

function splitTopLevelSpans(maskedCode, start, end, delimiter = ",") {
  const spans = [];
  let depth = 0;
  let segStart = start;
  for (let i = start; i < end; i += 1) {
    const ch = maskedCode[i];
    if (ch === "(" || ch === "[" || ch === "{") {
      depth += 1;
    } else if (ch === ")" || ch === "]" || ch === "}") {
      if (depth > 0) depth -= 1;
    } else if (depth === 0 && ch === delimiter) {
      spans.push({ start: segStart, end: i });
      segStart = i + 1;
    }
  }
  spans.push({ start: segStart, end });
  return spans;
}

function findExpressionEnd(source, maskedCode, exprStart, continueOnPipe = false) {
  let depth = 0;
  for (let i = exprStart; i < maskedCode.length; i += 1) {
    const ch = maskedCode[i];
    if (ch === "(" || ch === "[" || ch === "{") {
      depth += 1;
      continue;
    }
    if (ch === ")" || ch === "]" || ch === "}") {
      if (depth === 0) return i;
      depth -= 1;
      continue;
    }
    if (depth === 0) {
      if (ch === ";" || ch === ",") return i;
      if (ch === "\n") {
        let prev = i - 1;
        while (prev >= exprStart && /[ \t\r\n]/.test(source[prev])) prev -= 1;
        if (prev < exprStart) continue;
        const prevCh = maskedCode[prev];
        if ("=+-?:&|,([{!".includes(prevCh)) continue;
        let next = i + 1;
        while (next < maskedCode.length && /\s/.test(maskedCode[next])) next += 1;
        if (next >= maskedCode.length) return i;
        const nextTwo = maskedCode.slice(next, next + 2);
        const nextCh = maskedCode[next];
        if (
          nextCh === "." ||
          nextCh === "?" ||
          nextCh === ":" ||
          nextCh === "+" ||
          nextCh === ")" ||
          nextCh === "]" ||
          nextTwo === "&&" ||
          nextTwo === "||" ||
          nextTwo === "??" ||
          (continueOnPipe && (nextCh === "|" || nextCh === "&"))
        ) {
          continue;
        }
        return i;
      }
    }
  }
  return maskedCode.length;
}

function isPositionInRanges(pos, ranges) {
  return ranges.some((r) => pos >= r.start && pos < r.end);
}

function getPrevIdentifier(maskedCode, endInclusive) {
  let k = endInclusive;
  while (k >= 0 && /[A-Za-z0-9_$]/.test(maskedCode[k])) k -= 1;
  return maskedCode.slice(k + 1, endInclusive + 1);
}

function isPrecededByFunctionKeyword(maskedCode, identStart) {
  let prev = identStart - 1;
  while (prev >= 0 && /\s/.test(maskedCode[prev])) prev -= 1;
  if (prev < 0) return false;
  return getPrevIdentifier(maskedCode, prev) === "function";
}

function isCallDeclaration(maskedCode, identStart, closeParen) {
  if (isPrecededByFunctionKeyword(maskedCode, identStart)) return true;
  const after = maskedCode.slice(closeParen + 1);
  if (/^\s*(?::[^{=;\n]+)?\s*\{/.test(after)) return true;
  return false;
}

function isDirectArgOfCall(maskedCode, pos, calleeName) {
  let depth = 0;
  for (let i = pos - 1; i >= 0; i -= 1) {
    const ch = maskedCode[i];
    if (ch === ")" || ch === "]" || ch === "}") {
      depth += 1;
    } else if (ch === "(" || ch === "[" || ch === "{") {
      if (depth > 0) {
        depth -= 1;
      } else if (ch === "(") {
        let prev = i - 1;
        while (prev >= 0 && /\s/.test(maskedCode[prev])) prev -= 1;
        if (prev < 0) return false;
        return getPrevIdentifier(maskedCode, prev) === calleeName;
      } else {
        return false;
      }
    }
  }
  return false;
}

function hasTopLevelSemicolon(maskedCode, openBrace, closeBrace) {
  let depth = 0;
  for (let i = openBrace + 1; i < closeBrace; i += 1) {
    const ch = maskedCode[i];
    if (ch === "(" || ch === "[" || ch === "{") depth += 1;
    else if (ch === ")" || ch === "]" || ch === "}") depth -= 1;
    else if (depth === 0 && ch === ";") return true;
  }
  return false;
}

function isLiteralAtExpressionLevel(maskedCode, rangeStart, rangeEnd, lit) {
  if (lit.start < rangeStart || lit.end > rangeEnd) return false;
  const frames = [];
  for (let i = rangeStart; i < lit.start; i += 1) {
    const ch = maskedCode[i];
    if (ch === "(") {
      let prev = i - 1;
      while (prev >= rangeStart && /\s/.test(maskedCode[prev])) prev -= 1;
      let isCall = false;
      if (prev >= rangeStart) {
        const prevCh = maskedCode[prev];
        if (prevCh === ")" || prevCh === "]") {
          isCall = true;
        } else if (/[A-Za-z0-9_$]/.test(prevCh)) {
          const word = getPrevIdentifier(maskedCode, prev);
          isCall = !GROUPING_PREFIX_KEYWORDS.has(word);
        }
      }
      frames.push(isCall);
    } else if (ch === "{") {
      frames.push(true);
    } else if (ch === "[") {
      let prev = i - 1;
      while (prev >= rangeStart && /\s/.test(maskedCode[prev])) prev -= 1;
      let isSubscript = false;
      if (prev >= rangeStart) {
        const prevCh = maskedCode[prev];
        if (prevCh === ")" || prevCh === "]") {
          isSubscript = true;
        } else if (/[A-Za-z0-9_$]/.test(prevCh)) {
          const word = getPrevIdentifier(maskedCode, prev);
          isSubscript = !GROUPING_PREFIX_KEYWORDS.has(word);
        }
      }
      frames.push(isSubscript);
    } else if (ch === ")" || ch === "]" || ch === "}") {
      if (frames.length > 0) frames.pop();
    }
  }
  if (frames.some(Boolean)) return false;
  const beforeTrimmed = maskedCode.slice(rangeStart, lit.start).trimEnd();
  if (/(?:===|!==|==|!=)$/.test(beforeTrimmed)) return false;
  const afterTrimmed = maskedCode.slice(lit.end, rangeEnd).trimStart();
  if (/^(?:===|!==|==|!=)/.test(afterTrimmed)) return false;
  return true;
}

function collectTsTypeAndAllowedRanges(source, maskedCode) {
  const ignoredTypeRanges = [];
  const allowedRawRanges = [];

  // TypeScript interface blocks
  const ifaceRx = /(?:^|[;\n\r{}])\s*(?:export\s+)?interface\s+[A-Za-z_$][\w$]*/g;
  for (const m of maskedCode.matchAll(ifaceRx)) {
    let i = (m.index ?? 0) + m[0].length;
    let angleDepth = 0;
    let parenDepth = 0;
    while (i < maskedCode.length) {
      const ch = maskedCode[i];
      if (ch === "<") angleDepth += 1;
      else if (ch === ">" && angleDepth > 0) angleDepth -= 1;
      else if (ch === "(") parenDepth += 1;
      else if (ch === ")" && parenDepth > 0) parenDepth -= 1;
      else if (ch === "{" && angleDepth === 0 && parenDepth === 0) {
        const closeBrace = findMatchingDelimiter(maskedCode, i);
        if (closeBrace !== -1) {
          ignoredTypeRanges.push({ start: i, end: closeBrace + 1 });
        }
        break;
      } else if (ch === ";" && angleDepth === 0 && parenDepth === 0) {
        break;
      }
      i += 1;
    }
  }

  // TypeScript type aliases
  const typeRx = /(?:^|[;\n\r{}])\s*(?:export\s+)?type\s+[A-Za-z_$][\w$]*/g;
  for (const m of maskedCode.matchAll(typeRx)) {
    let i = (m.index ?? 0) + m[0].length;
    let angleDepth = 0;
    while (i < maskedCode.length) {
      const ch = maskedCode[i];
      if (ch === "<") angleDepth += 1;
      else if (ch === ">" && angleDepth > 0) angleDepth -= 1;
      else if (ch === "=" && angleDepth === 0 && maskedCode[i + 1] !== ">") {
        const exprEnd = findExpressionEnd(source, maskedCode, i + 1, true);
        ignoredTypeRanges.push({ start: i + 1, end: exprEnd + 1 });
        break;
      } else if (ch === ";" || ch === "\n") {
        if (angleDepth === 0) break;
      }
      i += 1;
    }
  }

  // Full-argument allowed technical/DOM/console calls
  const fullArgCallRx =
    /(?:\b(?:new\s+[A-Za-z_$][\w$]*Error|createUi2TechnicalDetails|renderUi2TechnicalCode|createUi2Icon|createElement|createElementNS|querySelector|querySelectorAll|closest|getElementById|getElementsByClassName|getElementsByTagName|getAttribute|hasAttribute|removeAttribute|matches)|(?:\bconsole\.(?:log|warn|error|info|debug))|(?:\.classList\.(?:add|remove|toggle|contains|replace)))\s*\(/g;
  for (const m of maskedCode.matchAll(fullArgCallRx)) {
    const matchStart = m.index ?? 0;
    const openParen = matchStart + m[0].length - 1;
    const closeParen = findMatchingDelimiter(maskedCode, openParen);
    if (closeParen === -1) continue;
    if (!m[0].startsWith(".") && !/\bnew\s+/.test(m[0]) && !m[0].startsWith("console.")) {
      if (isCallDeclaration(maskedCode, matchStart, closeParen)) continue;
    }
    allowedRawRanges.push({ start: openParen + 1, end: closeParen });
  }

  // i18n calls: exempt key arg[0] for t/tPlural and format arg[1] for formatDate/formatNumber
  const i18nCallRx = /(?:^|[^\w$.]|\b(?:[A-Za-z_$][\w$]*\.)?i18n\.)(t|tPlural|formatDate|formatNumber)\s*\(/g;
  for (const m of maskedCode.matchAll(i18nCallRx)) {
    const fnName = m[1];
    const openParen = (m.index ?? 0) + m[0].length - 1;
    const closeParen = findMatchingDelimiter(maskedCode, openParen);
    if (closeParen === -1) continue;
    if (isCallDeclaration(maskedCode, (m.index ?? 0) + m[0].indexOf(fnName), closeParen)) continue;
    const args = splitTopLevelSpans(maskedCode, openParen + 1, closeParen);
    if (fnName === "t" || fnName === "tPlural") {
      if (args.length >= 1) allowedRawRanges.push(args[0]);
    } else if (fnName === "formatDate" || fnName === "formatNumber") {
      if (args.length >= 2) {
        allowedRawRanges.push({ start: args[1].start, end: closeParen });
      }
    }
  }

  return { ignoredTypeRanges, allowedRawRanges };
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

  // Rule 3b & Rule 3c: Forbid raw UI copy in option objects, DOM properties, and UI helper calls
  if (!isCatalog && !isIcons) {
    const { ignoredTypeRanges, allowedRawRanges } = collectTsTypeAndAllowedRanges(content, maskedCode);
    const reportedRawStarts = new Set();

    function flagRawLiteral(lit, detail) {
      if (!/[A-Za-z]{2,}/.test(lit.staticText)) return;
      if (isPositionInRanges(lit.start, allowedRawRanges)) return;
      if (reportedRawStarts.has(lit.start)) return;
      reportedRawStarts.add(lit.start);
      violations.push({
        code: "MP_UI2_RULE_RAW_STRING",
        file: relPath,
        line: lineNumberAt(content, lit.start),
        detail,
      });
    }

    function checkExprLiterals(rangeStart, rangeEnd, messageForLiteral) {
      for (const lit of literals) {
        if (lit.start < rangeStart) continue;
        if (lit.start >= rangeEnd) break;
        if (lit.end > rangeEnd) continue;
        if (isLiteralAtExpressionLevel(maskedCode, rangeStart, rangeEnd, lit)) {
          flagRawLiteral(lit, messageForLiteral(lit));
        }
      }
    }

    // 1) Component / option object literal properties
    for (let i = 0; i < maskedCode.length; i += 1) {
      if (maskedCode[i] !== "{") continue;
      if (isPositionInRanges(i, ignoredTypeRanges)) continue;
      const closeBrace = findMatchingDelimiter(maskedCode, i);
      if (closeBrace === -1) continue;
      if (hasTopLevelSemicolon(maskedCode, i, closeBrace)) continue;

      const inStatCard = isDirectArgOfCall(maskedCode, i, "createUi2StatCard");
      const props = splitTopLevelSpans(maskedCode, i + 1, closeBrace);
      for (const prop of props) {
        const slice = maskedCode.slice(prop.start, prop.end);
        const keyMatch = slice.match(/^\s*([A-Za-z_$][\w$]*)\s*:/);
        if (!keyMatch) continue;
        const propKey = keyMatch[1];
        if (!UI_COPY_OPTION_KEYS.has(propKey) && !(inStatCard && propKey === "value")) {
          continue;
        }
        const valStart = prop.start + keyMatch[0].length;
        checkExprLiterals(
          valStart,
          prop.end,
          (lit) =>
            `Raw UI string "${lit.staticText.trim()}" in UI option "${propKey}"; use i18n.t(...).`
        );
      }
    }

    // 2) Direct DOM text/label property assignments across full RHS expression
    for (const m of maskedCode.matchAll(
      /\.(textContent|innerText|placeholder|title|innerHTML|ariaLabel)\s*(?:\+=|=)(?![=>])/g
    )) {
      const propName = m[1];
      const exprStart = (m.index ?? 0) + m[0].length;
      const exprEnd = findExpressionEnd(content, maskedCode, exprStart);
      checkExprLiterals(
        exprStart,
        exprEnd,
        (lit) =>
          `Raw UI string "${lit.staticText.trim()}" assigned directly to DOM property "${propName}"; use i18n.t(...).`
      );
    }

    // 3) Element.setAttribute("aria-label" | "title" | "placeholder" | "alt", <expr>)
    for (const m of maskedCode.matchAll(/\.setAttribute\s*\(/g)) {
      const openParen = (m.index ?? 0) + m[0].length - 1;
      const closeParen = findMatchingDelimiter(maskedCode, openParen);
      if (closeParen === -1) continue;
      const args = splitTopLevelSpans(maskedCode, openParen + 1, closeParen);
      if (args.length < 2) continue;
      const attrLit = literals.find(
        (lit) =>
          lit.start >= args[0].start &&
          lit.end <= args[0].end &&
          maskedCode.slice(args[0].start, lit.start).trim() === "" &&
          maskedCode.slice(lit.end, args[0].end).trim() === ""
      );
      if (!attrLit || !DOM_UI_ATTRS.has(attrLit.staticText.toLowerCase())) continue;
      checkExprLiterals(
        args[1].start,
        args[1].end,
        (lit) =>
          `Raw literal "${lit.staticText.trim()}" passed to setAttribute("${attrLit.staticText}"); use i18n.t(...).`
      );
    }

    // 4) Element.insertAdjacentText(pos, <expr>) / insertAdjacentHTML(pos, <expr>)
    for (const m of maskedCode.matchAll(/\.insertAdjacent(?:Text|HTML)\s*\(/g)) {
      const openParen = (m.index ?? 0) + m[0].length - 1;
      const closeParen = findMatchingDelimiter(maskedCode, openParen);
      if (closeParen === -1) continue;
      const args = splitTopLevelSpans(maskedCode, openParen + 1, closeParen);
      if (args.length < 2) continue;
      checkExprLiterals(
        args[1].start,
        args[1].end,
        (lit) =>
          `Raw UI string "${lit.staticText.trim()}" passed to insertAdjacent*; use i18n.t(...).`
      );
    }

    // 5) toast.show(<expr>) / field.setError(<expr>)
    for (const m of maskedCode.matchAll(/\.(show|setError)\s*\(/g)) {
      const methodName = m[1];
      const openParen = (m.index ?? 0) + m[0].length - 1;
      const closeParen = findMatchingDelimiter(maskedCode, openParen);
      if (closeParen === -1) continue;
      const args = splitTopLevelSpans(maskedCode, openParen + 1, closeParen);
      if (args.length < 1 || !/\S/.test(content.slice(args[0].start, args[0].end))) continue;
      checkExprLiterals(
        args[0].start,
        args[0].end,
        (lit) =>
          `Raw UI string "${lit.staticText.trim()}" passed to .${methodName}(); use i18n.t(...).`
      );
    }

    // 6) createUi2LoadingSkeleton(doc, <ariaLabelExpr>, ...)
    for (const m of maskedCode.matchAll(/(?:^|[^\w$.])createUi2LoadingSkeleton\s*\(/g)) {
      const matchPos = m.index ?? 0;
      const fnPos = matchPos + m[0].indexOf("createUi2LoadingSkeleton");
      const openParen = matchPos + m[0].length - 1;
      const closeParen = findMatchingDelimiter(maskedCode, openParen);
      if (closeParen === -1 || isCallDeclaration(maskedCode, fnPos, closeParen)) continue;
      const args = splitTopLevelSpans(maskedCode, openParen + 1, closeParen);
      if (args.length < 2) continue;
      checkExprLiterals(
        args[1].start,
        args[1].end,
        (lit) =>
          `Raw UI string "${lit.staticText.trim()}" passed to createUi2LoadingSkeleton(); use i18n.t(...).`
      );
    }

    // 7) DOM .append(...) / .prepend(...) / .replaceChildren(...) direct literals/ternaries
    for (const m of maskedCode.matchAll(/\.(append|prepend|replaceChildren)\s*\(/g)) {
      const methodName = m[1];
      const openParen = (m.index ?? 0) + m[0].length - 1;
      const closeParen = findMatchingDelimiter(maskedCode, openParen);
      if (closeParen === -1) continue;
      const args = splitTopLevelSpans(maskedCode, openParen + 1, closeParen);
      for (const arg of args) {
        checkExprLiterals(
          arg.start,
          arg.end,
          (lit) =>
            `Raw UI string "${lit.staticText.trim()}" passed to .${methodName}(); use i18n.t(...).`
        );
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

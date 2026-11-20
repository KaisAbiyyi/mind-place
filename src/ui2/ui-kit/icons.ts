export const UI2_ICON_NAMES = [
  "home",
  "chat",
  "memory",
  "map",
  "folder",
  "file",
  "link",
  "sliders",
  "search",
  "lock",
  "panel",
  "info",
  "copy",
  "download",
  "plus",
  "x",
  "check",
  "chev",
  "right",
  "back",
  "note",
  "list",
  "grid",
] as const;

export type Ui2IconName = (typeof UI2_ICON_NAMES)[number];

const SVG_NS = "http://www.w3.org/2000/svg";
const SPRITE_ID = "ui2-icon-sprite";

const SYMBOL_PATHS: Record<Ui2IconName, string> = {
  home: `<path d="M3 10.5L12 3l9 7.5V20a1 1 0 0 1-1 1h-5.5v-6h-5v6H4a1 1 0 0 1-1-1z"/>`,
  chat: `<path d="M4 5.5A1.5 1.5 0 0 1 5.5 4h13A1.5 1.5 0 0 1 20 5.5v9a1.5 1.5 0 0 1-1.5 1.5H9l-4.5 3.5V16H5.5A1.5 1.5 0 0 1 4 14.5z"/>`,
  memory: `<circle cx="12" cy="8.5" r="3.5"/><path d="M6.5 18.5c0-2.5 2.5-4 5.5-4s5.5 1.5 5.5 4"/><path d="M5 9.5a2 2 0 1 0 0-4"/><path d="M19 9.5a2 2 0 1 1 0-4"/>`,
  map: `<circle cx="6.5" cy="7" r="2.2"/><circle cx="17.5" cy="7" r="2.2"/><circle cx="12" cy="17" r="2.5"/><path d="M8.5 8.2l7 0M7.8 8.9l3.1 5.8M16.2 8.9l-3.1 5.8"/>`,
  folder: `<path d="M3.5 6.5A1.5 1.5 0 0 1 5 5h4.2l1.8 2H19a1.5 1.5 0 0 1 1.5 1.5v9A1.5 1.5 0 0 1 19 19H5a1.5 1.5 0 0 1-1.5-1.5z"/>`,
  file: `<path d="M6 3.5h8l4 4V20a.5.5 0 0 1-.5.5h-11A.5.5 0 0 1 6 20z"/><path d="M14 3.5V7.5h4"/>`,
  link: `<path d="M10 13.5a3.5 3.5 0 0 0 5 0l3-3a3.5 3.5 0 0 0-5-5l-1.3 1.3"/><path d="M14 10.5a3.5 3.5 0 0 0-5 0l-3 3a3.5 3.5 0 0 0 5 5l1.3-1.3"/>`,
  sliders: `<path d="M4 7h16M4 12h16M4 17h16"/><circle cx="8.5" cy="7" r="1.8" fill="currentColor"/><circle cx="15.5" cy="12" r="1.8" fill="currentColor"/><circle cx="10.5" cy="17" r="1.8" fill="currentColor"/>`,
  search: `<circle cx="11" cy="11" r="6"/><path d="M20 20l-4.2-4.2"/>`,
  lock: `<rect x="5" y="10.5" width="14" height="9.5" rx="2"/><path d="M8 10.5V7.5a4 4 0 0 1 8 0v3"/>`,
  panel: `<rect x="3.5" y="4.5" width="17" height="15" rx="2"/><path d="M9.5 4.5v15"/>`,
  info: `<circle cx="12" cy="12" r="8.5"/><path d="M12 11v5"/><circle cx="12" cy="8" r=".7" fill="currentColor"/>`,
  copy: `<rect x="9" y="9" width="10" height="10" rx="1.8"/><path d="M6 15H5a1.5 1.5 0 0 1-1.5-1.5V5A1.5 1.5 0 0 1 5 3.5h8.5A1.5 1.5 0 0 1 15 5v1"/>`,
  download: `<path d="M12 4v10m0 0l-4-4m4 4l4-4"/><path d="M4.5 16.5v2a1.5 1.5 0 0 0 1.5 1.5h12a1.5 1.5 0 0 0 1.5-1.5v-2"/>`,
  plus: `<path d="M12 5v14M5 12h14"/>`,
  x: `<path d="M6 6l12 12M18 6L6 18"/>`,
  check: `<path d="M5 12.5l4.5 4.5L19 7"/>`,
  chev: `<path d="M7 10l5 5 5-5"/>`,
  right: `<path d="M9 6l6 6-6 6"/>`,
  back: `<path d="M15 6l-6 6 6 6"/>`,
  note: `<rect x="4" y="4" width="16" height="16" rx="2"/><path d="M8 9h8M8 13h8M8 17h5"/>`,
  list: `<path d="M5 7h14M5 12h14M5 17h14"/>`,
  grid: `<rect x="4" y="4" width="6.5" height="6.5" rx="1.2"/><rect x="13.5" y="4" width="6.5" height="6.5" rx="1.2"/><rect x="4" y="13.5" width="6.5" height="6.5" rx="1.2"/><rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.2"/>`,
};

export function ensureUi2IconSprite(doc: Document): SVGSVGElement {
  const existing = doc.getElementById(SPRITE_ID);
  if (existing && existing.tagName.toLowerCase() === "svg") {
    return existing as unknown as SVGSVGElement;
  }
  const svg = doc.createElementNS(SVG_NS, "svg");
  svg.setAttribute("id", SPRITE_ID);
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("hidden", "");
  svg.style.position = "absolute";
  svg.style.width = "0";
  svg.style.height = "0";
  svg.style.overflow = "hidden";

  const symbols = UI2_ICON_NAMES.map(
    (name) =>
      `<symbol id="ui2-i-${name}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">${SYMBOL_PATHS[name]}</symbol>`
  ).join("");
  svg.innerHTML = symbols;
  (doc.body ?? doc.documentElement).prepend(svg);
  return svg;
}

export function createUi2Icon(doc: Document, name: Ui2IconName, sizePx = 16): SVGSVGElement {
  ensureUi2IconSprite(doc);
  const svg = doc.createElementNS(SVG_NS, "svg");
  svg.setAttribute("width", String(sizePx));
  svg.setAttribute("height", String(sizePx));
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("aria-hidden", "true");
  svg.classList.add("ui2-icon");
  const use = doc.createElementNS(SVG_NS, "use");
  use.setAttribute("href", `#ui2-i-${name}`);
  svg.append(use);
  return svg;
}

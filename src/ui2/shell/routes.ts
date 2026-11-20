import type { Ui2TranslationKey } from "../i18n/index.js";
import { UI2_DESTINATIONS, type Ui2Destination, type Ui2IconName } from "../ui-kit/index.js";

export interface Ui2RouteEntry {
  readonly destination: Ui2Destination;
  readonly path: string;
  readonly labelKey: Ui2TranslationKey;
  readonly icon: Ui2IconName;
}

export interface Ui2BreadcrumbStep {
  readonly path: string;
  readonly labelKey: Ui2TranslationKey;
}

export interface Ui2ResolvedRoute {
  readonly path: string;
  readonly destination: Ui2Destination;
  readonly breadcrumbs: readonly Ui2BreadcrumbStep[];
  readonly available: boolean;
  readonly recognized: boolean;
}

/** Stable shell route table. Downstream pages own their content, never this navigation contract. */
export const UI2_SHELL_DESTINATIONS: readonly Ui2RouteEntry[] = [
  { destination: "home", path: "/home", labelKey: "nav.home", icon: "home" },
  { destination: "memory", path: "/memory", labelKey: "nav.memory", icon: "memory" },
  { destination: "archive", path: "/archive", labelKey: "nav.archive", icon: "chat" },
  { destination: "constellation", path: "/constellation", labelKey: "nav.constellation", icon: "map" },
  { destination: "sources", path: "/sources", labelKey: "nav.sources", icon: "folder" },
  { destination: "connections", path: "/connections", labelKey: "nav.connections", icon: "link" },
  { destination: "settings", path: "/settings", labelKey: "nav.settings", icon: "sliders" },
] as const;

/** Only known static child paths: no corpus keys, secrets, or raw identities in URLs. */
export const UI2_SHELL_CHILDREN = [
  { path: "/memory/detail", destination: "memory", labelKey: "action.showDetails" },
  { path: "/archive/chat", destination: "archive", labelKey: "action.openChat" },
  { path: "/sources/add", destination: "sources", labelKey: "home.quick.addSource" },
  { path: "/settings/appearance", destination: "settings", labelKey: "settings.theme" },
] as const;

const HOME: Ui2BreadcrumbStep = { path: "/home", labelKey: "nav.home" };

function normalizeRoutePath(path: string): string {
  const clean = path.split("?")[0]?.split("#")[0] ?? "";
  if (!clean || clean === "/") return "/home";
  if (!/^\/[a-z/-]+$/.test(clean)) return "";
  return clean.replace(/\/$/, "");
}

export function resolveUi2Route(input: string): Ui2ResolvedRoute {
  const path = normalizeRoutePath(input);
  const destination = UI2_SHELL_DESTINATIONS.find((item) => item.path === path);
  if (destination) {
    return {
      path, destination: destination.destination,
      breadcrumbs: destination.destination === "home" ? [HOME] : [HOME, { path, labelKey: destination.labelKey }],
      available: false, recognized: true,
    };
  }
  const child = UI2_SHELL_CHILDREN.find((item) => item.path === path);
  if (child) {
    const parent = UI2_SHELL_DESTINATIONS.find((item) => item.destination === child.destination)!;
    return {
      path, destination: parent.destination,
      breadcrumbs: [HOME, { path: parent.path, labelKey: parent.labelKey }, { path, labelKey: child.labelKey }],
      available: false, recognized: true,
    };
  }
  return { path: "/home", destination: "home", breadcrumbs: [HOME], available: false, recognized: false };
}

/** UI2 URLs contain only allowlisted static paths. They never disclose record IDs. */
export const UI2_HASH_PREFIX = "#ui2=";
export function readUi2Hash(hash: string): Ui2ResolvedRoute {
  if (!hash.startsWith(UI2_HASH_PREFIX)) return resolveUi2Route("/home");
  try { return resolveUi2Route(decodeURIComponent(hash.slice(UI2_HASH_PREFIX.length))); }
  catch { return resolveUi2Route(""); }
}

export function ui2HashForPath(path: string): string {
  return UI2_HASH_PREFIX + encodeURIComponent(resolveUi2Route(path).path);
}

if (UI2_SHELL_DESTINATIONS.length !== UI2_DESTINATIONS.length ||
    UI2_SHELL_DESTINATIONS.some((entry, index) => entry.destination !== UI2_DESTINATIONS[index])) {
  throw new Error("UI2_SHELL_DESTINATION_ORDER_MISMATCH");
}

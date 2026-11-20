export const UI2_FONT_SIZES_PX = [12, 13, 14, 16, 20, 28] as const;
export const UI2_FONT_WEIGHTS = [400, 500, 600] as const;
export const UI2_SPACING_PX = [4, 8, 12, 16, 24, 32, 48] as const;
export const UI2_RADII_PX = [7, 10, 14] as const;
export const UI2_SETTINGS_MAX_ROWS_PER_CARD = 7 as const;

export const UI2_DESTINATIONS = [
  "home",
  "memory",
  "archive",
  "constellation",
  "sources",
  "connections",
  "settings",
] as const;

export type Ui2Destination = (typeof UI2_DESTINATIONS)[number];

export const UI2_MAP_NAMES = [
  "Constellation",
  "Atlas",
  "Mindscape",
  "Synapse",
  "Hive",
] as const;

export type Ui2MapName = (typeof UI2_MAP_NAMES)[number];
export type Ui2ThemeMode = "system" | "light" | "dark";
export type Ui2DensityMode = "comfortable" | "compact";

export const UI2_LIGHT_TOKENS = {
  bg: "#fafafa",
  surface: "#ffffff",
  sunken: "#f4f4f5",
  line: "#e4e4e7",
  lineStrong: "#d4d4d8",
  fg: "#18181b",
  fg2: "#52525b",
  fg3: "#8b8b94",
  accent: "#18181b",
  onAccent: "#fafafa",
  warnBg: "#fef6e7",
  warnFg: "#8a5a00",
  ok: "#1f7a4d",
  okBg: "#edf7f1",
  danger: "#9f2d2d",
  dangerBg: "#fdf2f2",
  onDanger: "#fafafa",
} as const;

export const UI2_DARK_TOKENS = {
  bg: "#09090b",
  surface: "#111113",
  sunken: "#18181b",
  line: "#27272a",
  lineStrong: "#3f3f46",
  fg: "#f4f4f5",
  fg2: "#a1a1aa",
  fg3: "#71717a",
  accent: "#f4f4f5",
  onAccent: "#09090b",
  warnBg: "#2a2210",
  warnFg: "#e8c170",
  ok: "#5ec796",
  okBg: "#12261c",
  danger: "#fca5a5",
  dangerBg: "#2a1414",
  onDanger: "#09090b",
} as const;

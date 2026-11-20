export const UI2_SHELL_FEATURE_FLAG = "mind_place.ui2.shell.preview";

export interface Ui2FlagStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem?(key: string): void;
}

function browserStorage(): Ui2FlagStorage | undefined {
  try {
    return globalThis.localStorage;
  } catch {
    return undefined;
  }
}

/** Owner/dev opt-in only. Defaults to v1, even if storage is missing or corrupt. */
export function isUi2ShellEnabled(storage: Ui2FlagStorage | undefined = browserStorage()): boolean {
  try {
    return storage?.getItem(UI2_SHELL_FEATURE_FLAG) === "enabled";
  } catch {
    return false;
  }
}

export function setUi2ShellEnabled(enabled: boolean, storage: Ui2FlagStorage | undefined = browserStorage()): boolean {
  if (!storage) return false;
  try {
    storage.setItem(UI2_SHELL_FEATURE_FLAG, enabled ? "enabled" : "disabled");
    return true;
  } catch {
    return false;
  }
}
